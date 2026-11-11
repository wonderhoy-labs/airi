package com.wonderhoylabs.airi.linux

import com.wonderhoylabs.airi.launcher.AppBuilder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LinuxTest {
	@Test
	fun keysRoundTrip() {
		assertEquals("terminal", Linux.Terminal.key)
		assertEquals("minori:0b6f-1", Linux.Machine("0b6f-1").key)
		assertEquals(Linux.Terminal, Linux.fromKey("terminal"))
		assertEquals(Linux.Machine("0b6f-1"), Linux.fromKey("minori:0b6f-1"))
		assertNull(Linux.fromKey("minori:"))
		assertNull(Linux.fromKey("termux"))
	}

	@Test
	fun terminalAppsKeepTheirPackages() {
		// Airi 0.3.0 named this app's package so, and the launcher apps already made must still be found.
		assertEquals(
			"com.wonderhoylabs.airi.linux.a_org_gnome_calculator_" + sha8("org.gnome.Calculator.desktop"),
			AppBuilder.packageFor(Linux.Terminal, "org.gnome.Calculator.desktop"),
		)
	}

	@Test
	fun eachMachineGetsItsOwnPackage() {
		val terminal = AppBuilder.packageFor(Linux.Terminal, "foot.desktop")
		val one = AppBuilder.packageFor(Linux.Machine("one"), "foot.desktop")
		val two = AppBuilder.packageFor(Linux.Machine("two"), "foot.desktop")
		assertNotEquals(terminal, one)
		assertNotEquals(one, two)
		assertEquals("com.wonderhoylabs.airi.linux.m_foot_" + sha8("minori:one/foot.desktop"), one)
	}

	private fun sha8(text: String) =
		java.security.MessageDigest.getInstance("SHA-256").digest(text.toByteArray()).joinToString("") {
			"%02x".format(it)
		}.take(8)
}
