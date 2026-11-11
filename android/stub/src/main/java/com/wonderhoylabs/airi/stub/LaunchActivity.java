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
 * <p>The link is the same one the client library's {@code Airi.openIntent} makes. It's written out here so the apps
 * carry no library code.
 *
 * <p>It stays underneath Airi's window instead of finishing, because Android names a task in Recents
 * after the activity at its root. That's what makes this show up as the Linux app, not as Airi.
 */
public final class LaunchActivity extends Activity {
	private static final String AIRI = "com.wonderhoylabs.airi";
	private static final int VIEWER = 1;

	private boolean reopened;

	@Override
	protected void onCreate(Bundle state) {
		super.onCreate(state);
		// Coming back after Android reclaimed this process, with the window still open above it.
		if (state == null) open();
	}

	private void open() {
		try {
			String desktopId = asset("desktop_id");
			String machine = machine();
			Uri.Builder link = new Uri.Builder().scheme("airi");
			if (machine == null) {
				link.authority("app").appendPath(desktopId);
			} else {
				link.authority("minori").appendPath(machine).appendPath(desktopId);
			}
			Intent intent = new Intent(Intent.ACTION_VIEW, link.build());
			intent.setClassName(AIRI, AIRI + ".viewer.ViewerActivity");
			startActivityForResult(intent, VIEWER);
		} catch (IOException | ActivityNotFoundException e) {
			Toast.makeText(this, "This app needs Airi. Install it, then try again.", Toast.LENGTH_LONG).show();
			finish();
		}
	}

	private String asset(String name) throws IOException {
		try (InputStream in = getAssets().open(name)) {
			return new String(in.readAllBytes(), StandardCharsets.UTF_8).trim();
		}
	}

	/** The Minori machine the app is in. Apps for Android's terminal have no such asset. */
	private String machine() {
		try {
			return asset("machine");
		} catch (IOException e) {
			return null;
		}
	}

	/**
	 * Airi says when a window closed normally, and that leaves nothing to do. Otherwise Airi was updated
	 * or stopped underneath this, and someone has just tapped the icon, so it has to open again. Once is
	 * enough, in case that's what stopped it.
	 */
	@Override
	protected void onActivityResult(int requestCode, int resultCode, Intent data) {
		if (resultCode == RESULT_OK || reopened) {
			finish();
		} else {
			reopened = true;
			open();
		}
	}
}
