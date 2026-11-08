package com.wonderhoylabs.airi.launcher

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class BinaryManifestTest {
	/** Builds a compiled XML file holding these strings, followed by a chunk that has to survive. */
	private fun manifest(strings: List<String>, tail: ByteArray): ByteArray {
		val text = strings.flatMap { value ->
			val chars = value.flatMap { listOf(it.code.toByte(), (it.code ushr 8).toByte()) }
			listOf(value.length.toByte(), 0.toByte()) + chars + listOf<Byte>(0, 0)
		}.toMutableList()
		while (text.size % 4 != 0) text.add(0)
		val stringsStart = 28 + strings.size * 4
		val poolSize = stringsStart + text.size
		val total = 8 + poolSize + tail.size
		val out = ByteBuffer.allocate(total).order(ByteOrder.LITTLE_ENDIAN)
		out.putShort(3).putShort(8).putInt(total)
		out.putShort(1).putShort(28).putInt(poolSize).putInt(strings.size).putInt(0).putInt(0)
		out.putInt(stringsStart).putInt(0)
		var offset = 0
		strings.forEach {
			out.putInt(offset)
			offset += 2 + it.length * 2 + 2
		}
		out.put(text.toByteArray())
		out.put(tail)
		return out.array()
	}

	@Test
	fun replacesOnlyWhatItWasAsked() {
		val tail = byteArrayOf(1, 2, 3, 4, 5, 6, 7, 8)
		val before = manifest(listOf("package", "old.name", "label"), tail)
		val after = BinaryManifest.replaceStrings(before, mapOf("old.name" to "a.much.longer.package.name"))
		val expected = manifest(listOf("package", "a.much.longer.package.name", "label"), tail)
		assertArrayEquals(expected, after)
	}

	@Test
	fun handlesNamesOutsideLatin() {
		val before = manifest(listOf("label"), ByteArray(0))
		val after = BinaryManifest.replaceStrings(before, mapOf("label" to "計算機"))
		assertArrayEquals(manifest(listOf("計算機"), ByteArray(0)), after)
	}

	@Test
	fun updatesTheTotalSize() {
		val before = manifest(listOf("a"), ByteArray(4))
		val after = BinaryManifest.replaceStrings(before, mapOf("a" to "bbbbbbbbbbbbbbbb"))
		assertEquals(after.size, ByteBuffer.wrap(after).order(ByteOrder.LITTLE_ENDIAN).getInt(4))
	}

	@Test(expected = IllegalArgumentException::class)
	fun refusesAStringThatIsNotThere() {
		BinaryManifest.replaceStrings(manifest(listOf("a"), ByteArray(0)), mapOf("missing" to "b"))
	}

	@Test(expected = IllegalArgumentException::class)
	fun refusesAnythingElse() {
		BinaryManifest.replaceStrings(ByteArray(64), mapOf("a" to "b"))
	}
}
