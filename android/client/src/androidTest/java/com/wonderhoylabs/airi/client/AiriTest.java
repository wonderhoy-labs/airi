package com.wonderhoylabs.airi.client;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assume.assumeTrue;

import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import androidx.test.platform.app.InstrumentationRegistry;
import java.util.List;
import org.junit.Test;

/**
 * The links run anywhere. The rest needs Airi on the device, and an app it put on the launcher, or the template
 * that those are made from: {@code adb install android/stub/build/outputs/apk/release/stub-release.apk}.
 */
public final class AiriTest {
	private final Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();

	@Test
	public void linksRoundTrip() {
		Uri uri = Airi.appUri("org.gnome.Calculator.desktop");
		assertEquals("airi://app/org.gnome.Calculator.desktop", uri.toString());
		assertEquals("org.gnome.Calculator.desktop", Airi.desktopId(uri));
		// Desktop file IDs can have characters that need escaping.
		assertEquals("my app+1.desktop", Airi.desktopId(Airi.appUri("my app+1.desktop")));
	}

	@Test
	public void minoriLinksRoundTrip() {
		Uri uri = Airi.minoriAppUri("0b6f2c1e-1111-4a4a-9c9c-0123456789ab", "foot.desktop");
		assertEquals("airi://minori/0b6f2c1e-1111-4a4a-9c9c-0123456789ab/foot.desktop", uri.toString());
		assertEquals("foot.desktop", Airi.desktopId(uri));
		assertEquals("0b6f2c1e-1111-4a4a-9c9c-0123456789ab", Airi.minoriMachine(uri));
		assertNull(Airi.minoriMachine(Airi.appUri("foot.desktop")));
		assertNull(Airi.desktopId(Uri.parse("airi://minori/foot.desktop")));
		assertNull(Airi.minoriMachine(Uri.parse("airi://minori/a/b/c")));
	}

	@Test
	public void openMinoriIntentGoesToTheViewer() {
		Intent intent = Airi.openMinoriIntent("vm", "foot.desktop");
		assertEquals(Airi.PACKAGE, intent.getComponent().getPackageName());
		assertEquals("vm", Airi.minoriMachine(intent.getData()));
	}

	@Test
	public void otherLinksAreNotApps() {
		assertNull(Airi.desktopId(null));
		assertNull(Airi.desktopId(Uri.parse("https://app/org.gnome.Calculator.desktop")));
		assertNull(Airi.desktopId(Uri.parse("airi://window/org.gnome.Calculator.desktop")));
		assertNull(Airi.desktopId(Uri.parse("airi://app/")));
		assertNull(Airi.desktopId(Uri.parse("airi://app/a/b")));
	}

	@Test
	public void openIntentGoesToTheViewer() {
		Intent intent = Airi.openIntent("org.gnome.Calculator.desktop");
		assertEquals(Intent.ACTION_VIEW, intent.getAction());
		assertEquals(Airi.PACKAGE, intent.getComponent().getPackageName());
		assertEquals("org.gnome.Calculator.desktop", Airi.desktopId(intent.getData()));
	}

	@Test
	public void airiTakesTheIntent() {
		assumeTrue("Airi isn't installed", Airi.isInstalled(context));
		PackageManager packages = context.getPackageManager();
		assertNotNull(packages.resolveActivity(Airi.openIntent("org.gnome.Calculator.desktop"), 0));
		// Any app can also open the link without naming Airi.
		Intent link = new Intent(Intent.ACTION_VIEW, Airi.appUri("org.gnome.Calculator.desktop"));
		assertNotNull(packages.resolveActivity(link, 0));
		assertTrue(Airi.canOpenMinoriApps(context));
	}

	@Test
	public void findsLauncherApps() {
		List<LinuxApp> apps = Airi.launcherApps(context);
		assumeTrue("Nothing on the launcher from Airi", !apps.isEmpty());
		PackageManager packages = context.getPackageManager();
		for (LinuxApp app : apps) {
			assertFalse(app.desktopId.isEmpty());
			assertFalse(app.label.isEmpty());
			assertTrue(app.packageName.startsWith("com.wonderhoylabs.airi.linux."));
			assertNotNull(packages.resolveActivity(app.launchIntent(), 0));
			assertNotNull(app.loadIcon(context));
			assertEquals(app.packageName, Airi.launcherApp(context, app.machineId, app.desktopId).packageName);
		}
		assertNull(Airi.launcherApp(context, "not.an.app.desktop"));
	}
}
