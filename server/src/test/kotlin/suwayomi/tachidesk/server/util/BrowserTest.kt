package suwayomi.tachidesk.server.util

import org.junit.jupiter.api.AfterEach
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import suwayomi.tachidesk.graphql.types.WebUIInterface
import suwayomi.tachidesk.server.serverConfig
import suwayomi.tachidesk.test.ApplicationTest
import java.nio.file.Files

class BrowserTest : ApplicationTest() {
    @AfterEach
    fun resetSettings() {
        serverConfig.webUIInterface.value = WebUIInterface.BROWSER
        serverConfig.tauriPath.value = ""
        @Suppress("DEPRECATION")
        run {
            serverConfig.electronPath.value = ""
        }
        serverConfig.ip.value = "0.0.0.0"
        serverConfig.port.value = 4567
        serverConfig.webUISubpath.value = ""
    }

    @Test
    fun resolvesElectronInterfaceAsTauri() {
        serverConfig.webUIInterface.value = WebUIInterface.ELECTRON

        assertEquals(WebUIInterface.TAURI, Browser.resolveInterface())
    }

    @Test
    fun prefersTauriPathOverElectronPath() {
        val tauriFile = Files.createTempFile("tauri-launcher", ".bin").toFile()
        val electronFile = Files.createTempFile("electron-launcher", ".bin").toFile()

        serverConfig.tauriPath.value = tauriFile.absolutePath
        @Suppress("DEPRECATION")
        run {
            serverConfig.electronPath.value = electronFile.absolutePath
        }

        assertEquals(tauriFile.absolutePath, Browser.resolveLauncherPath())
    }

    @Test
    fun usesElectronPathAsDeprecatedFallback() {
        val electronFile = Files.createTempFile("electron-launcher", ".bin").toFile()

        serverConfig.tauriPath.value = ""
        @Suppress("DEPRECATION")
        run {
            serverConfig.electronPath.value = electronFile.absolutePath
        }

        assertEquals(electronFile.absolutePath, Browser.resolveLauncherPath())
    }

    @Test
    fun computesBaseUrlWithSubpath() {
        serverConfig.ip.value = "0.0.0.0"
        serverConfig.port.value = 1234
        serverConfig.webUISubpath.value = "/ui"

        val baseUrl = Browser.getAppBaseUrl()
        assertTrue(baseUrl.startsWith("http://127.0.0.1:1234"))
        assertTrue(baseUrl.endsWith("/ui"))
    }
}
