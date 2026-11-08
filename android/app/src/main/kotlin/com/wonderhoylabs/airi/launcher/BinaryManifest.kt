package com.wonderhoylabs.airi.launcher

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Changes strings in a compiled AndroidManifest.xml. A compiled manifest keeps its text in one table
 * and refers to it by position, so swapping text means rebuilding that table and nothing else.
 */
object BinaryManifest {
	private const val XML_CHUNK = 0x0003
	private const val STRING_POOL_CHUNK = 0x0001
	private const val STRING_POOL_HEADER = 28
	private const val UTF8_FLAG = 0x100

	fun replaceStrings(manifest: ByteArray, replacements: Map<String, String>): ByteArray {
		val input = ByteBuffer.wrap(manifest).order(ByteOrder.LITTLE_ENDIAN)
		require(input.getShort(0).toInt() == XML_CHUNK) { "This isn't a compiled manifest" }
		val xmlHeaderSize = input.getShort(2).toInt() and 0xffff
		require(input.getShort(xmlHeaderSize).toInt() == STRING_POOL_CHUNK) { "The string table isn't first" }

		val poolSize = input.getInt(xmlHeaderSize + 4)
		val strings = readStrings(input, xmlHeaderSize)
		val missing = replacements.keys - strings.toSet()
		require(missing.isEmpty()) { "The manifest doesn't contain $missing" }

		val pool = writeStrings(strings.map { replacements[it] ?: it })
		val total = manifest.size - poolSize + pool.size
		val out = ByteBuffer.allocate(total).order(ByteOrder.LITTLE_ENDIAN)
		out.put(manifest, 0, xmlHeaderSize)
		out.putInt(4, total)
		out.put(pool)
		out.put(manifest, xmlHeaderSize + poolSize, manifest.size - xmlHeaderSize - poolSize)
		return out.array()
	}

	private fun readStrings(data: ByteBuffer, pool: Int): List<String> {
		val count = data.getInt(pool + 8)
		val styles = data.getInt(pool + 12)
		val flags = data.getInt(pool + 16)
		val start = pool + data.getInt(pool + 20)
		require(styles == 0) { "Manifests don't have styled text" }
		return List(count) { index ->
			val at = start + data.getInt(pool + STRING_POOL_HEADER + index * 4)
			if (flags and UTF8_FLAG != 0) readUtf8(data, at) else readUtf16(data, at)
		}
	}

	private fun readUtf16(data: ByteBuffer, at: Int): String {
		var position = at
		var length = data.getShort(position).toInt() and 0xffff
		position += 2
		if (length and 0x8000 != 0) {
			length = (length and 0x7fff shl 16) or (data.getShort(position).toInt() and 0xffff)
			position += 2
		}
		return String(CharArray(length) { data.getChar(position + it * 2) })
	}

	private fun readUtf8(data: ByteBuffer, at: Int): String {
		var position = at
		// The character count comes first and isn't needed here.
		if (data.get(position).toInt() and 0x80 != 0) position++
		position++
		var length = data.get(position++).toInt() and 0xff
		if (length and 0x80 != 0) length = (length and 0x7f shl 8) or (data.get(position++).toInt() and 0xff)
		return String(ByteArray(length) { data.get(position + it) })
	}

	/** Writes a string table in UTF-16, which is valid whichever one the template used. */
	private fun writeStrings(strings: List<String>): ByteArray {
		val text = ByteArrayOutputStream()
		val offsets = strings.map { value ->
			val offset = text.size()
			text.writeLe16Length(value.length)
			value.forEach { text.writeLe16(it.code) }
			text.writeLe16(0)
			offset
		}
		while (text.size() % 4 != 0) text.write(0)

		val stringsStart = STRING_POOL_HEADER + strings.size * 4
		val size = stringsStart + text.size()
		val out = ByteBuffer.allocate(size).order(ByteOrder.LITTLE_ENDIAN)
		out.putShort(STRING_POOL_CHUNK.toShort())
		out.putShort(STRING_POOL_HEADER.toShort())
		out.putInt(size)
		out.putInt(strings.size)
		out.putInt(0)
		out.putInt(0)
		out.putInt(stringsStart)
		out.putInt(0)
		offsets.forEach(out::putInt)
		out.put(text.toByteArray())
		return out.array()
	}

	private fun ByteArrayOutputStream.writeLe16(value: Int) {
		write(value)
		write(value ushr 8)
	}

	private fun ByteArrayOutputStream.writeLe16Length(length: Int) {
		if (length >= 0x8000) writeLe16(length ushr 16 or 0x8000)
		writeLe16(length and 0xffff)
	}
}
