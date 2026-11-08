package com.wonderhoylabs.airi.launcher

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Typeface
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import com.android.apksig.ApkSigner
import java.io.ByteArrayOutputStream
import java.io.File
import java.math.BigInteger
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.PrivateKey
import java.security.cert.X509Certificate
import java.util.Calendar
import java.util.zip.CRC32
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream
import java.util.zip.ZipOutputStream
import javax.security.auth.x500.X500Principal

/**
 * Makes the small app that puts one Linux app on the launcher. It starts from a template built with
 * Airi, and changes four things: its package, its name, its icon, and which Linux app it opens.
 */
class AppBuilder(private val context: Context) {
	fun build(desktopId: String, name: String, iconPng: ByteArray?, output: File) {
		val unsigned = File.createTempFile("app", ".apk", context.cacheDir)
		try {
			writeUnsigned(desktopId, name, iconPng, unsigned)
			sign(unsigned, output)
		} finally {
			unsigned.delete()
		}
	}

	private fun writeUnsigned(desktopId: String, name: String, iconPng: ByteArray?, output: File) {
		val replaced = mutableSetOf<String>()
		ZipInputStream(context.assets.open(TEMPLATE).buffered()).use { template ->
			ZipOutputStream(output.outputStream().buffered()).use { apk ->
				while (true) {
					val entry = template.nextEntry ?: break
					// The template's own signature can't survive these changes.
					if (entry.name.startsWith("META-INF/")) continue
					val original = template.readBytes()
					val data = when {
						entry.name == MANIFEST -> BinaryManifest.replaceStrings(
							original,
							mapOf(TEMPLATE_PACKAGE to packageFor(desktopId), TEMPLATE_LABEL to name),
						).also { replaced += MANIFEST }

						entry.name == DESKTOP_ID -> desktopId.toByteArray().also { replaced += DESKTOP_ID }

						// The build renames resources, but the icon is the template's only image.
						entry.name.startsWith("res/") && entry.name.endsWith(".png") ->
							iconForeground(name, iconPng).also { replaced += ICON }

						else -> original
					}
					// Android reads the resource table straight out of the file, so it can't be compressed.
					writeEntry(apk, entry.name, data, stored = entry.name == "resources.arsc")
				}
			}
		}
		// An app that installs but opens the wrong thing, or has no icon, is worse than an error.
		check(replaced == setOf(MANIFEST, DESKTOP_ID, ICON)) { "The template changed: only found $replaced" }
	}

	private fun writeEntry(apk: ZipOutputStream, name: String, data: ByteArray, stored: Boolean) {
		val entry = ZipEntry(name)
		if (stored) {
			entry.method = ZipEntry.STORED
			entry.size = data.size.toLong()
			entry.compressedSize = data.size.toLong()
			entry.crc = CRC32().apply { update(data) }.value
		}
		apk.putNextEntry(entry)
		apk.write(data)
		apk.closeEntry()
	}

	/** Places the icon in the part of an adaptive icon that no launcher's mask can cut off. */
	private fun iconForeground(name: String, iconPng: ByteArray?): ByteArray {
		val bitmap = Bitmap.createBitmap(ICON_SIZE, ICON_SIZE, Bitmap.Config.ARGB_8888)
		val canvas = Canvas(bitmap)
		val icon = iconPng?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
		val margin = (ICON_SIZE - ICON_SAFE_SIZE) / 2f
		val area = RectF(margin, margin, ICON_SIZE - margin, ICON_SIZE - margin)
		if (icon != null) {
			val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
			canvas.drawBitmap(icon, Rect(0, 0, icon.width, icon.height), area, paint)
		} else {
			drawLetter(canvas, area, name)
		}
		return ByteArrayOutputStream().also { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }.toByteArray()
	}

	/** Stands in for an app with no icon that can be read. */
	private fun drawLetter(canvas: Canvas, area: RectF, name: String) {
		val paint = Paint(Paint.ANTI_ALIAS_FLAG)
		paint.color = LETTER_BACKGROUNDS[Math.floorMod(name.hashCode(), LETTER_BACKGROUNDS.size)]
		canvas.drawRoundRect(area, area.width() / 4, area.width() / 4, paint)
		paint.color = Color.WHITE
		paint.typeface = Typeface.DEFAULT_BOLD
		paint.textSize = area.height() * 0.6f
		paint.textAlign = Paint.Align.CENTER
		val letter = name.trim().take(1).uppercase().ifEmpty { "?" }
		canvas.drawText(letter, area.centerX(), area.centerY() - (paint.ascent() + paint.descent()) / 2, paint)
	}

	private fun sign(unsigned: File, output: File) {
		val (key, certificate) = signingKey()
		val signer = ApkSigner.SignerConfig.Builder("airi", key, listOf(certificate)).build()
		ApkSigner.Builder(listOf(signer))
			.setInputApk(unsigned)
			.setOutputApk(output)
			.setMinSdkVersion(MIN_SDK)
			.setV1SigningEnabled(false)
			.setV2SigningEnabled(true)
			.setV3SigningEnabled(false)
			.build()
			.sign()
	}

	/**
	 * Gets this phone's signing key, making one the first time. Android keeps it in hardware, and
	 * Android only updates an app that's signed with the same key, so it has to stay the same.
	 */
	private fun signingKey(): Pair<PrivateKey, X509Certificate> {
		val store = KeyStore.getInstance(KEY_STORE).apply { load(null) }
		if (!store.containsAlias(KEY_ALIAS)) {
			val notAfter = Calendar.getInstance().apply { add(Calendar.YEAR, 50) }.time
			val spec = KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_SIGN)
				.setKeySize(2048)
				.setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA512)
				.setSignaturePaddings(KeyProperties.SIGNATURE_PADDING_RSA_PKCS1)
				.setCertificateSubject(X500Principal("CN=Airi"))
				.setCertificateSerialNumber(BigInteger.ONE)
				.setCertificateNotAfter(notAfter)
				.build()
			KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_RSA, KEY_STORE).apply {
				initialize(spec)
				generateKeyPair()
			}
		}
		val entry = store.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
		return entry.privateKey to entry.certificate as X509Certificate
	}

	companion object {
		private const val TEMPLATE = "app-template.apk"
		private const val MANIFEST = "AndroidManifest.xml"
		private const val DESKTOP_ID = "assets/desktop_id"
		private const val ICON = "icon"
		private const val TEMPLATE_PACKAGE = "com.wonderhoylabs.airi.linux.template"
		private const val TEMPLATE_LABEL = "AIRI_APP_LABEL_PLACEHOLDER"
		private const val KEY_STORE = "AndroidKeyStore"
		private const val KEY_ALIAS = "airi-launcher-apps"
		private const val MIN_SDK = 35
		private const val ICON_SIZE = 432

		// An adaptive icon is 108 units across, and only the middle 66 are always shown.
		private const val ICON_SAFE_SIZE = ICON_SIZE * 66 / 108
		private val LETTER_BACKGROUNDS = intArrayOf(
			0xFFC2185B.toInt(),
			0xFF7B1FA2.toInt(),
			0xFF303F9F.toInt(),
			0xFF00796B.toInt(),
			0xFFE64A19.toInt(),
		)

		/** Gives each Linux app a package of its own that stays the same across rebuilds. */
		fun packageFor(desktopId: String): String {
			val readable = desktopId.removeSuffix(".desktop").lowercase()
				.replace(Regex("[^a-z0-9]+"), "_").trim('_').take(40)
			val hash = MessageDigest.getInstance("SHA-256").digest(desktopId.toByteArray()).toHexString().take(8)
			return "com.wonderhoylabs.airi.linux.a_${readable}_$hash"
		}
	}
}
