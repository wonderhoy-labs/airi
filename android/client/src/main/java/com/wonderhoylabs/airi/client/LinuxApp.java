package com.wonderhoylabs.airi.client;

import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.drawable.Drawable;

/** A Linux app that Airi has put on the launcher, as an Android app of its own. */
public final class LinuxApp {
	/** The desktop file ID of the Linux app, such as {@code org.gnome.Calculator.desktop}. */
	public final String desktopId;

	/** Its name on the launcher. */
	public final String label;

	/** The package of the launcher app Airi made for it. */
	public final String packageName;

	private final String activity;

	LinuxApp(String desktopId, String label, String packageName, String activity) {
		this.desktopId = desktopId;
		this.label = label;
		this.packageName = packageName;
		this.activity = activity;
	}

	/** Opens it the way its launcher icon does, so it has one task, with its own name and icon in Recents. */
	public Intent launchIntent() {
		return new Intent(Intent.ACTION_MAIN)
				.addCategory(Intent.CATEGORY_LAUNCHER)
				.setClassName(packageName, activity)
				.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED);
	}

	/** Its launcher icon, which is the Linux app's own. Null if the app has gone since it was found. */
	public Drawable loadIcon(Context context) {
		try {
			return context.getPackageManager().getApplicationIcon(packageName);
		} catch (PackageManager.NameNotFoundException e) {
			return null;
		}
	}

	@Override
	public String toString() {
		return "LinuxApp(" + desktopId + ", " + label + ", " + packageName + ")";
	}
}
