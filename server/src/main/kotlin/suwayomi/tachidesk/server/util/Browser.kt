package suwayomi.tachidesk.server.util

/*
 * Copyright (C) Contributors to the Suwayomi project
 *
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import dorkbox.desktop.Desktop
import io.github.oshai.kotlinlogging.KotlinLogging
import suwayomi.tachidesk.graphql.types.WebUIInterface
import suwayomi.tachidesk.server.serverConfig

object Browser {
    private val logger = KotlinLogging.logger { }
    private val launcherInstances = mutableListOf<Any>()

    internal fun getAppBaseUrl(): String {
        val appIP = if (serverConfig.ip.value == "0.0.0.0") "127.0.0.1" else serverConfig.ip.value
        val baseUrl = "http://$appIP:${serverConfig.port.value}"

        return ServerSubpath.maybeAddAsSuffix(baseUrl)
    }

    internal fun resolveInterface(): WebUIInterface =
        when (serverConfig.webUIInterface.value) {
            WebUIInterface.ELECTRON -> {
                logger.warn { "server.webUIInterface=electron is deprecated, treating as tauri" }
                WebUIInterface.TAURI
            }
            else -> serverConfig.webUIInterface.value
        }

    @Suppress("DEPRECATION")
    internal fun resolveLauncherPath(): String? {
        val tauriPath = serverConfig.tauriPath.value.trim()
        if (tauriPath.isNotEmpty()) {
            return tauriPath
        }

        val electronPath = serverConfig.electronPath.value.trim()
        if (electronPath.isNotEmpty()) {
            logger.warn { "server.electronPath is deprecated, using it as fallback for tauriPath" }
            return electronPath
        }

        return null
    }

    fun openInBrowser() {
        if (serverConfig.webUIEnabled.value) {
            val appBaseUrl = getAppBaseUrl()

            if (resolveInterface() == WebUIInterface.TAURI) {
                try {
                    val launcherPath = resolveLauncherPath()
                    if (launcherPath.isNullOrBlank()) {
                        logger.warn { "tauri launcher path is empty, falling back to default browser" }
                        Desktop.browseURL(appBaseUrl)
                        return
                    }

                    launcherInstances.add(ProcessBuilder(launcherPath, appBaseUrl).start())
                } catch (e: Throwable) {
                    // cover both java.lang.Exception and java.lang.Error
                    logger.error(e) { "openInBrowser: failed to launch tauri, falling back to browser" }
                    try {
                        Desktop.browseURL(appBaseUrl)
                    } catch (fallbackError: Throwable) {
                        logger.error(fallbackError) { "openInBrowser: failed to launch browser fallback due to" }
                    }
                }
            } else {
                try {
                    Desktop.browseURL(appBaseUrl)
                } catch (e: Throwable) {
                    // cover both java.lang.Exception and java.lang.Error
                    logger.error(e) { "openInBrowser: failed to launch browser due to" }
                }
            }
        }
    }
}
