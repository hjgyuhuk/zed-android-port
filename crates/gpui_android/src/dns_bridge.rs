//! JNI bridge that reads Android's currently active DNS servers.
//!
//! Android has no `/etc/resolv.conf`; it routes DNS through `netd` and
//! the Java `ConnectivityManager` API. The system's own programs (ssh
//! among them) resolve through netd and follow the network on their own;
//! this bridge only reports the servers for logging and diagnostics.
//!
//! Falls back to public DNS (1.1.1.1, 8.8.8.8) when `ConnectivityManager`
//! reports no active network, which happens during boot before WiFi
//! attaches, or when the user is offline.

use android_activity::AndroidApp;
use anyhow::{Context, Result};
use jni::JNIEnv;
use jni::objects::{JClass, JString};

const FALLBACK_NAMESERVERS: &[&str] = &["1.1.1.1", "8.8.8.8"];

/// Active-network DNS servers, or the public fallback when Android
/// reports none. Never empty.
pub fn android_dns_servers(android_app: &AndroidApp) -> Vec<String> {
    match query_android_dns(android_app) {
        Ok(servers) if !servers.is_empty() => servers,
        Ok(_) => {
            log::info!(
                "dns_bridge: ConnectivityManager returned no DNS servers \
                 (no active network?); falling back to public DNS"
            );
            fallback_nameservers()
        }
        Err(err) => {
            log::warn!(
                "dns_bridge: getActiveDnsServers failed: {err:#}; falling back to public DNS"
            );
            fallback_nameservers()
        }
    }
}

fn fallback_nameservers() -> Vec<String> {
    FALLBACK_NAMESERVERS.iter().map(|s| s.to_string()).collect()
}

fn query_android_dns(android_app: &AndroidApp) -> Result<Vec<String>> {
    crate::jni_env::with_activity(android_app, |env, activity| {
        let result = env
            .call_method(activity, "getActiveDnsServers", "()Ljava/lang/String;", &[])
            .context("MainActivity.getActiveDnsServers")?;
        let result_obj = result
            .l()
            .context("getActiveDnsServers returned non-object")?;
        if result_obj.is_null() {
            return Ok(Vec::new());
        }
        let jstr: JString = result_obj.into();
        let csv: String = env
            .get_string(&jstr)
            .context("decode getActiveDnsServers result")?
            .into();
        Ok(csv
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect())
    })
}

/// `NativeBridge.nativeDnsServersChanged`, on Android's connectivity thread.
/// The system resolver (netd) follows the network on its own; the change is
/// only logged.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_zdroid_NativeBridge_nativeDnsServersChanged<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    servers: JString<'local>,
) {
    let servers: String = match env.get_string(&servers) {
        Ok(servers) => servers.into(),
        Err(error) => {
            log::warn!("dns_bridge: decode the DNS servers: {error}");
            return;
        }
    };
    log::debug!("dns_bridge: active DNS servers now {servers}");
}
