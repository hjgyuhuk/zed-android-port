//! Standalone Android-native helper for Zed's SSH_ASKPASS flow.
//!
//! On desktop, Zed sets `ASKPASS_PROGRAM` (in `crates/askpass/src/askpass.rs`)
//! to `current_exe()` — i.e. the same `zed` binary, invoked with `--askpass=
//! <socket>`. The askpass.sh shim pipes ssh's prompt into that invocation,
//! the binary connects to a unix socket back to the running Zed, the main
//! process pops a UI modal asking for the password, and the password flows
//! back through the socket and onto stdout where ssh reads it.
//!
//! On Android, `current_exe()` resolves to `/system/bin/app_process64` (the
//! Zygote / Dalvik launcher that hosts the APK's main DEX runtime). When
//! ssh execs that binary outside of an Activity context, Android aborts
//! with `Error changing dalvik-cache ownership: Permission denied` —
//! the app's SELinux domain can't re-spawn another instance of the
//! Zygote. The askpass attempt SIGABRTs three times, ssh treats it as
//! three failed password tries, and connection ends with `Permission
//! denied (publickey,password)` even though the user never got a chance
//! to type anything.
//!
//! This helper is the missing piece: a tiny, regular ELF (not Zygote /
//! not Dalvik / not app_process) that does the same socket-IPC dance the
//! askpass crate's `main()` does on desktop. It ships in the APK as
//! `libaskpass.so` in the native library dir — the one app-owned location
//! the kernel may exec from at any targetSdk — and is wired in via
//! `askpass::set_askpass_program` so any subsequent AskPassSession uses
//! this binary instead of `current_exe()`. ssh execs it directly with the
//! prompt as argv[1]; the socket path arrives as ZED_ASKPASS_SOCKET.
//!
//! Protocol (kept identical to `crates/askpass/src/askpass.rs::main` so
//! the existing socket server in the gpui process Just Works):
//!
//!   1. Parse `--askpass=<unix-socket-path>` from argv.
//!   2. Read all of stdin into a buffer (this is the prompt ssh wants
//!      answered, e.g. `"<user>@<host>'s password: "`).
//!   3. NUL-terminate the buffer if it isn't already (the desktop main
//!      function does this — keep the byte stream identical).
//!   4. Connect to the unix socket. Write the prompt buffer.
//!   5. Read the socket until EOF — this is the password (or
//!      passphrase) the user typed into Zed's modal.
//!   6. Write that response to stdout. ssh reads stdout and treats it
//!      as the password.
//!
//! It also stands in for `zed --printenv`, which Zed's login shell capture
//! (`util::shell_env::capture`) runs inside the shell to read its
//! environment: `util::get_shell_safe_zed_path` returns this helper on
//! Android (`zdroid_hooks::register_zed_helper`).

use std::collections::HashMap;
use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

fn main() -> ExitCode {
    if env::args().skip(1).any(|argument| argument == "--printenv") {
        print_env();
        return ExitCode::SUCCESS;
    }

    // ssh execs SSH_ASKPASS directly with the prompt as argv[1] and no way
    // to pass extra arguments, so the socket path arrives as ZED_ASKPASS_SOCKET
    // (as on Windows). The --askpass= form stays for the desktop-style script
    // flow, which pipes the prompt into stdin instead.
    let mut prompts = Vec::new();
    let mut socket = None;
    for argument in env::args().skip(1) {
        match argument.strip_prefix("--askpass=") {
            Some(value) if !value.is_empty() => socket = Some(value.to_string()),
            _ => prompts.push(argument),
        }
    }
    let socket = socket.or_else(|| env::var("ZED_ASKPASS_SOCKET").ok());
    let socket = match socket {
        Some(socket) if !socket.is_empty() => socket,
        _ => {
            eprintln!(
                "zed-askpass-helper: no --askpass=<socket> argument or ZED_ASKPASS_SOCKET environment variable"
            );
            return ExitCode::from(2);
        }
    };

    let mut stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("zed-askpass-helper: connect {socket}: {err}");
            return ExitCode::from(1);
        }
    };

    let mut prompt = if prompts.is_empty() {
        let mut prompt = Vec::new();
        if let Err(err) = io::stdin().read_to_end(&mut prompt) {
            eprintln!("zed-askpass-helper: read stdin: {err}");
            return ExitCode::from(1);
        }
        prompt
    } else {
        prompts.join("\0").into_bytes()
    };
    if prompt.last() != Some(&b'\0') {
        prompt.push(b'\0');
    }

    if let Err(err) = stream.write_all(&prompt) {
        eprintln!("zed-askpass-helper: write socket: {err}");
        return ExitCode::from(1);
    }
    // Half-close the write side so the server knows the prompt is
    // complete and can start replying. Without this, the server's
    // `read_to_end` would block forever waiting for more bytes.
    if let Err(err) = stream.shutdown(std::net::Shutdown::Write) {
        eprintln!("zed-askpass-helper: shutdown write: {err}");
        return ExitCode::from(1);
    }

    let mut response = Vec::new();
    if let Err(err) = stream.read_to_end(&mut response) {
        eprintln!("zed-askpass-helper: read socket: {err}");
        return ExitCode::from(1);
    }

    if let Err(err) = io::stdout().write_all(&response) {
        eprintln!("zed-askpass-helper: write stdout: {err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

/// Zed's `util::shell_env::print_env`, which `zed --printenv` runs.
fn print_env() {
    let env_vars: HashMap<String, String> = env::vars().collect();
    let json = serde_json::to_string_pretty(&env_vars).unwrap_or_else(|err| {
        eprintln!("Error serializing environment variables: {}", err);
        std::process::exit(1);
    });
    println!("{}", json);
}
