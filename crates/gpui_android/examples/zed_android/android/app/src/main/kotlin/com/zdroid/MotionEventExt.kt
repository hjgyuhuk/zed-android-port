package com.zdroid

import android.app.Activity
import android.view.InputDevice
import android.view.MotionEvent
import android.view.SurfaceView
import android.view.ViewGroup
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/// Whether Android recognized this as a gesture on a trackpad, such as a two-finger
/// scroll or a pinch, and reports it with fake fingers. The classifications of a
/// touchscreen's events have that source instead.
internal val MotionEvent.isTrackpadGesture: Boolean
    get() = isFromSource(InputDevice.SOURCE_MOUSE) &&
        classification != MotionEvent.CLASSIFICATION_NONE

/// Sends this event to Rust over JNI, for a window without GameActivity's input
/// queue, or one whose classification that queue would lose.
internal fun MotionEvent.forwardTo(windowId: Long) {
    if (pointerCount <= 0) return
    val xs = FloatArray(pointerCount)
    val ys = FloatArray(pointerCount)
    val ids = IntArray(pointerCount)
    val toolTypes = IntArray(pointerCount)
    for (i in 0 until pointerCount) {
        xs[i] = getX(i)
        ys[i] = getY(i)
        ids[i] = getPointerId(i)
        toolTypes[i] = getToolType(i)
    }
    NativeBridge.nativeOnMotionEvent(
        windowId,
        actionMasked,
        actionIndex,
        source,
        buttonState,
        metaState,
        flags,
        classification,
        // ACTION_SCROLL (the mouse wheel) carries its delta on these axes. They're
        // zero on other events, and Rust only reads them for a scroll.
        getAxisValue(MotionEvent.AXIS_VSCROLL),
        getAxisValue(MotionEvent.AXIS_HSCROLL),
        xs,
        ys,
        ids,
        toolTypes,
    )
}

/// Draws behind the system bars and the cutout, so gpui's surface gets the
/// whole window, and keeps the bars visible: the surface is margined inside
/// them ([insetBySafeArea]), so the editor never draws under them.
internal fun Activity.enterEdgeToEdge() {
    WindowCompat.setDecorFitsSystemWindows(window, false)
    WindowInsetsControllerCompat(window, window.decorView)
        .show(WindowInsetsCompat.Type.systemBars())
}

/// Margins the surface inside the system bars and the cutout, making gpui's
/// window the safe area. The margins use insets ignoring visibility so they
/// hold still while the keyboard covers the navigation bar; the part of the
/// keyboard above the surface's bottom edge is what Rust's layout avoids.
internal fun SurfaceView.insetBySafeArea() {
    ViewCompat.setOnApplyWindowInsetsListener(this) { view, insets ->
        val safe = insets.getInsetsIgnoringVisibility(
            WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        )
        val params = view.layoutParams as? ViewGroup.MarginLayoutParams
        if (params != null &&
            (params.leftMargin != safe.left || params.topMargin != safe.top ||
                params.rightMargin != safe.right || params.bottomMargin != safe.bottom)
        ) {
            params.setMargins(safe.left, safe.top, safe.right, safe.bottom)
            view.layoutParams = params
        }
        insets
    }
}
