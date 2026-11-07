package com.wonderhoylabs.airi.stub;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.widget.Toast;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

/**
 * Opens a Linux app in Airi. This is the only code in the apps Airi makes.
 *
 * <p>It stays underneath Airi's window instead of finishing, because Android names a task in Recents
 * after the activity at its root. That's what makes this show up as the Linux app, not as Airi.
 */
public final class LaunchActivity extends Activity {
    private static final String AIRI = "com.wonderhoylabs.airi";
    private static final int VIEWER = 1;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        // Coming back after Android reclaimed this process, with the window still open above it.
        if (state != null) return;
        try (InputStream in = getAssets().open("desktop_id")) {
            String desktopId = new String(in.readAllBytes(), StandardCharsets.UTF_8).trim();
            Intent intent = new Intent(Intent.ACTION_VIEW, new Uri.Builder()
                    .scheme("airi").authority("app").appendPath(desktopId).build());
            intent.setClassName(AIRI, AIRI + ".viewer.ViewerActivity");
            startActivityForResult(intent, VIEWER);
        } catch (IOException | ActivityNotFoundException e) {
            Toast.makeText(this, "This app needs Airi. Install it, then try again.", Toast.LENGTH_LONG).show();
            finish();
        }
    }

    /** The window closed, so this has nothing left to do. */
    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        finish();
    }
}
