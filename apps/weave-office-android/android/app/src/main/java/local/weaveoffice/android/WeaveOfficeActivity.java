package local.weaveoffice.android;

import android.os.Bundle;

import androidx.core.view.WindowCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.core.view.WindowInsetsControllerCompat;

import com.google.androidgamesdk.GameActivity;

/**
 * Weave Office's Activity: GameActivity runs the Rust app (libweave_office_android.so); this
 * class adds what winit doesn't offer on Android. The first layer is the window: the app draws
 * its own chrome, so the system bars are hidden and the surface runs edge to edge under a camera
 * cutout. The document pickers, the soft keyboard's height and the pen's tool type arrive in the
 * next layers (see docs/android/).
 */
public class WeaveOfficeActivity extends GameActivity {
    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        hideSystemBars();
        // Draw under a front camera's cutout on every Android version, as Android 15 does for
        // apps targeting it.
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.P) {
            android.view.WindowManager.LayoutParams lp = getWindow().getAttributes();
            lp.layoutInDisplayCutoutMode = android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R
                    ? android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS
                    : android.view.WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
            getWindow().setAttributes(lp);
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    private void hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(getWindow(), false);
        WindowInsetsControllerCompat controller =
                WindowCompat.getInsetsController(getWindow(), getWindow().getDecorView());
        controller.hide(WindowInsetsCompat.Type.systemBars());
        controller.setSystemBarsBehavior(
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
    }
}
