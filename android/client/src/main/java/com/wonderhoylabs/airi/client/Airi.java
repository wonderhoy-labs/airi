package com.wonderhoylabs.airi.client;

import android.content.Context;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.net.Uri;
import android.os.Build;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/**
 * Opens Linux apps in Airi from another Android app, and finds the ones Airi has put on the launcher.
 *
 * <p>A Linux app is named by its desktop file ID, such as {@code org.gnome.Calculator.desktop}: the name of its
 * {@code .desktop} file in Linux. Everything here works on any phone, and says so when Airi isn't installed.
 */
public final class Airi {
	/** Airi's package name. */
	public static final String PACKAGE = "com.wonderhoylabs.airi";

	/** What every launcher app Airi makes answers to. It's how they're found without seeing every app. */
	public static final String ACTION_LINUX_APP = "com.wonderhoylabs.airi.LINUX_APP";

	/** The scheme of the links that open Linux apps, {@code airi://app/<desktop file ID>}. */
	public static final String SCHEME = "airi";

	/** The host of the links that open Linux apps. */
	public static final String HOST = "app";

	/**
	 * The host of the links that open Linux apps in a Minori machine, {@code airi://minori/<machine ID>/<desktop file ID>}.
	 */
	public static final String MINORI_HOST = "minori";

	/** Where each launcher app keeps the desktop file ID of the Linux app it opens. */
	static final String DESKTOP_ID_ASSET = "desktop_id";

	/** Where a launcher app for a Minori machine's Linux app keeps the machine's ID. Apps for the terminal have none. */
	static final String MACHINE_ASSET = "machine";

	private static final String VIEWER = PACKAGE + ".viewer.ViewerActivity";

	private Airi() {}

	/** Whether Airi is installed. Linux apps also need Android's Linux terminal set up, which Airi walks through. */
	public static boolean isInstalled(Context context) {
		try {
			packageInfo(context.getPackageManager());
			return true;
		} catch (PackageManager.NameNotFoundException e) {
			return false;
		}
	}

	/** The link that opens a Linux app, such as {@code airi://app/org.gnome.Calculator.desktop}. */
	public static Uri appUri(String desktopId) {
		return new Uri.Builder().scheme(SCHEME).authority(HOST).appendPath(desktopId).build();
	}

	/**
	 * The link that opens a Linux app in one of Minori's machines, such as
	 * {@code airi://minori/0b6f…/org.gnome.Calculator.desktop}. The machine ID is the one Minori gives it, the {@code id}
	 * in its helper's {@code list}.
	 */
	public static Uri minoriAppUri(String machineId, String desktopId) {
		return new Uri.Builder().scheme(SCHEME).authority(MINORI_HOST).appendPath(machineId).appendPath(desktopId).build();
	}

	/**
	 * The desktop file ID in a link that {@link #appUri} or {@link #minoriAppUri} made, or null if it isn't one. Use
	 * {@link #minoriMachine} to tell which Linux it's in.
	 */
	public static String desktopId(Uri uri) {
		if (uri == null || !SCHEME.equals(uri.getScheme())) {
			return null;
		}
		List<String> segments = uri.getPathSegments();
		if (HOST.equals(uri.getAuthority())) {
			return segments.size() == 1 && !segments.get(0).isEmpty() ? segments.get(0) : null;
		}
		if (MINORI_HOST.equals(uri.getAuthority())) {
			return segments.size() == 2 && !segments.get(0).isEmpty() && !segments.get(1).isEmpty() ? segments.get(1) : null;
		}
		return null;
	}

	/** The Minori machine in a link that {@link #minoriAppUri} made, or null for Android's terminal or no link at all. */
	public static String minoriMachine(Uri uri) {
		if (desktopId(uri) == null || !MINORI_HOST.equals(uri.getAuthority())) {
			return null;
		}
		return uri.getPathSegments().get(0);
	}

	/**
	 * Opens a Linux app in a window of its own. Start it with {@code startActivity}, or with
	 * {@code startActivityForResult} to be told {@code RESULT_OK} when the window closed normally. Airi starts Linux
	 * first if it isn't running, which takes 30 to 45 seconds. Starting it when Airi isn't installed throws
	 * {@code ActivityNotFoundException}.
	 *
	 * <p>It opens in the task of the app that starts it, the way Airi's launcher apps do. Add
	 * {@code Intent.FLAG_ACTIVITY_NEW_DOCUMENT} to give it a task of its own, though then there's no result. For an app
	 * on the launcher, its {@link LinuxApp#launchIntent} is better: it has one task, with its own name in Recents.
	 */
	public static Intent openIntent(String desktopId) {
		return new Intent(Intent.ACTION_VIEW, appUri(desktopId)).setClassName(PACKAGE, VIEWER);
	}

	/**
	 * Opens a Linux app from one of Minori's machines, the way {@link #openIntent} does from Android's terminal. Airi
	 * reaches the machine through Minori, and asks Minori to start it if it's stopped. Check {@link #canOpenMinoriApps}
	 * first: an Airi older than 0.4.0 doesn't know these links.
	 */
	public static Intent openMinoriIntent(String machineId, String desktopId) {
		return new Intent(Intent.ACTION_VIEW, minoriAppUri(machineId, desktopId)).setClassName(PACKAGE, VIEWER);
	}

	/** Whether the installed Airi opens apps from Minori's machines, which it does from 0.4.0. */
	public static boolean canOpenMinoriApps(Context context) {
		Intent query = new Intent(Intent.ACTION_VIEW, minoriAppUri("machine", "app.desktop")).setPackage(PACKAGE);
		PackageManager packages = context.getPackageManager();
		return !(Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
				? packages.queryIntentActivities(query, PackageManager.ResolveInfoFlags.of(0))
				: queryIntentActivities(packages, query)).isEmpty();
	}

	/** The Linux apps Airi has put on the launcher, in no particular order. Empty if there are none, or no Airi. */
	public static List<LinuxApp> launcherApps(Context context) {
		PackageManager packages = context.getPackageManager();
		Intent query = new Intent(ACTION_LINUX_APP);
		List<ResolveInfo> found = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
				? packages.queryIntentActivities(query, PackageManager.ResolveInfoFlags.of(0))
				: queryIntentActivities(packages, query);
		if (found.isEmpty()) {
			return Collections.emptyList();
		}
		List<LinuxApp> apps = new ArrayList<>(found.size());
		for (ResolveInfo info : found) {
			ActivityInfo activity = info.activityInfo;
			String desktopId = readAsset(context, activity.packageName, DESKTOP_ID_ASSET);
			if (desktopId == null) {
				continue;
			}
			String machine = readAsset(context, activity.packageName, MACHINE_ASSET);
			apps.add(new LinuxApp(desktopId, machine, String.valueOf(info.loadLabel(packages)), activity.packageName,
					activity.name));
		}
		return apps;
	}

	/** The launcher app for a Linux app in Android's terminal, or null if it isn't on the launcher. */
	public static LinuxApp launcherApp(Context context, String desktopId) {
		return launcherApp(context, null, desktopId);
	}

	/**
	 * The launcher app for a Linux app in a Minori machine, or in Android's terminal if {@code machineId} is null. Null if
	 * it isn't on the launcher.
	 */
	public static LinuxApp launcherApp(Context context, String machineId, String desktopId) {
		for (LinuxApp app : launcherApps(context)) {
			boolean sameLinux = machineId == null ? app.machineId == null : machineId.equals(app.machineId);
			if (sameLinux && app.desktopId.equals(desktopId)) {
				return app;
			}
		}
		return null;
	}

	private static String readAsset(Context context, String packageName, String asset) {
		try {
			Context other = context.createPackageContext(packageName, 0);
			try (InputStream in = other.getAssets().open(asset)) {
				String id = new String(readAll(in), StandardCharsets.UTF_8).trim();
				return id.isEmpty() ? null : id;
			}
		} catch (PackageManager.NameNotFoundException | IOException e) {
			return null;
		}
	}

	// InputStream.readAllBytes only arrived in Android 13.
	private static byte[] readAll(InputStream in) throws IOException {
		ByteArrayOutputStream out = new ByteArrayOutputStream();
		byte[] buffer = new byte[256];
		for (int read; (read = in.read(buffer)) != -1;) {
			out.write(buffer, 0, read);
		}
		return out.toByteArray();
	}

	@SuppressWarnings("deprecation")
	private static void packageInfo(PackageManager packages) throws PackageManager.NameNotFoundException {
		if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
			packages.getPackageInfo(PACKAGE, PackageManager.PackageInfoFlags.of(0));
		} else {
			packages.getPackageInfo(PACKAGE, 0);
		}
	}

	@SuppressWarnings("deprecation")
	private static List<ResolveInfo> queryIntentActivities(PackageManager packages, Intent query) {
		return packages.queryIntentActivities(query, 0);
	}
}
