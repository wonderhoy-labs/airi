package com.wonderhoylabs.airi.launcher

import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import androidx.test.platform.app.InstrumentationRegistry
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.ZipFile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Builds a real app from the real template, and asks Android what it thinks of the result. */
class AppBuilderTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    private fun redSquare(): ByteArray {
        val bitmap = Bitmap.createBitmap(64, 64, Bitmap.Config.ARGB_8888).apply { eraseColor(Color.RED) }
        return ByteArrayOutputStream().also { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }.toByteArray()
    }

    private fun build(id: String, name: String, icon: ByteArray?): File {
        val apk = File.createTempFile("test", ".apk", context.cacheDir)
        AppBuilder(context).build(id, name, icon, apk)
        return apk
    }

    private fun foreground(apk: File): Bitmap = ZipFile(apk).use { zip ->
        val entry = zip.entries().asSequence().single { it.name.startsWith("res/") && it.name.endsWith(".png") }
        zip.getInputStream(entry).use { BitmapFactory.decodeStream(it) }
    }

    @Test
    fun androidReadsTheNameAndPackage() {
        val apk = build("org.example.Thing.desktop", "Thing 計算", null)
        val info = context.packageManager.getPackageArchiveInfo(apk.path, PackageManager.GET_SIGNING_CERTIFICATES)
        assertNotNull(info)
        assertEquals(AppBuilder.packageFor("org.example.Thing.desktop"), info!!.packageName)
        info.applicationInfo!!.sourceDir = apk.path
        info.applicationInfo!!.publicSourceDir = apk.path
        assertEquals("Thing 計算", info.applicationInfo!!.loadLabel(context.packageManager).toString())
        assertTrue(info.signingInfo!!.apkContentsSigners.isNotEmpty())
    }

    @Test
    fun theAppOpensTheRightLinuxApp() {
        val apk = build("org.example.Thing.desktop", "Thing", null)
        val id = ZipFile(apk).use { zip -> zip.getInputStream(zip.getEntry("assets/desktop_id")).readBytes() }
        assertEquals("org.example.Thing.desktop", String(id))
    }

    @Test
    fun theIconIsTheLinuxAppsIcon() {
        val icon = foreground(build("a.desktop", "A", redSquare()))
        assertEquals(Color.RED, icon.getPixel(icon.width / 2, icon.height / 2))
        // It stays inside the part of an adaptive icon that every launcher shows.
        assertEquals(Color.TRANSPARENT, icon.getPixel(4, 4))
    }

    @Test
    fun anAppWithNoIconStillGetsOne() {
        val icon = foreground(build("a.desktop", "Alpha", null))
        assertTrue(Color.alpha(icon.getPixel(icon.width / 4 + 30, icon.height / 2)) == 255)
    }

    @Test
    fun theTemplateStaysSmall() {
        // This gets copied for every Linux app, and a dependency sneaking in would make it megabytes.
        assertTrue(build("a.desktop", "A", redSquare()).length() < 200_000)
    }

    @Test
    fun packageNamesAreValidAndStable() {
        val name = AppBuilder.packageFor("Some App (Beta) 2.desktop")
        assertEquals(name, AppBuilder.packageFor("Some App (Beta) 2.desktop"))
        assertTrue(name, name.matches(Regex("[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+")))
        assertTrue(name != AppBuilder.packageFor("Some App Beta 2.desktop"))
    }
}
