package com.chronx.wallet

import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat

class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private var appWasInBackground = false
  private var backgroundedAt: Long = 0

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    WindowCompat.setDecorFitsSystemWindows(window, true)
  }

  override fun onPause() {
    super.onPause()
    appWasInBackground = true
    backgroundedAt = System.currentTimeMillis()
  }

  override fun onResume() {
    super.onResume()
    if (appWasInBackground) {
      appWasInBackground = false
      val elapsed = System.currentTimeMillis() - backgroundedAt
      if (elapsed > 5000) {
        triggerLockScreen()
      }
    }
  }

  private fun triggerLockScreen() {
    webView?.evaluateJavascript(
      "window.__chronxAppLocked && window.__chronxAppLocked()",
      null
    )
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    this.webView = webView
    webView.settings.setSupportZoom(false)
    webView.settings.builtInZoomControls = false
    webView.settings.displayZoomControls = false
    webView.settings.textZoom = 100
    webView.addJavascriptInterface(BiometricBridge(), "__chronxBiometric")
  }

  inner class BiometricBridge {
    @JavascriptInterface
    fun isAvailable(): String {
      val manager = BiometricManager.from(this@MainActivity)
      val canAuth = manager.canAuthenticate(
        BiometricManager.Authenticators.BIOMETRIC_WEAK or
        BiometricManager.Authenticators.DEVICE_CREDENTIAL
      )
      return when (canAuth) {
        BiometricManager.BIOMETRIC_SUCCESS -> "available"
        BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> "not_configured"
        BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE -> "not_supported"
        BiometricManager.BIOMETRIC_ERROR_HW_UNAVAILABLE -> "not_supported"
        else -> "not_supported"
      }
    }

    @JavascriptInterface
    fun authenticate() {
      runOnUiThread {
        val executor = ContextCompat.getMainExecutor(this@MainActivity)
        val callback = object : BiometricPrompt.AuthenticationCallback() {
          override fun onAuthenticationSucceeded(
            result: BiometricPrompt.AuthenticationResult
          ) {
            super.onAuthenticationSucceeded(result)
            webView?.evaluateJavascript(
              "window.__chronxBiometricResult && window.__chronxBiometricResult('success')",
              null
            )
          }
          override fun onAuthenticationError(
            errorCode: Int,
            errString: CharSequence
          ) {
            super.onAuthenticationError(errorCode, errString)
            val safe = errString.toString().replace("'", "\\'")
            webView?.evaluateJavascript(
              "window.__chronxBiometricResult && window.__chronxBiometricResult('error:$safe')",
              null
            )
          }
          override fun onAuthenticationFailed() {
            super.onAuthenticationFailed()
            // Don't report — user can retry fingerprint
          }
        }

        val prompt = BiometricPrompt(this@MainActivity, executor, callback)
        val promptInfo = BiometricPrompt.PromptInfo.Builder()
          .setTitle("ChronX Wallet")
          .setSubtitle("Verify your identity")
          .setAllowedAuthenticators(
            BiometricManager.Authenticators.BIOMETRIC_WEAK or
            BiometricManager.Authenticators.DEVICE_CREDENTIAL
          )
          .build()
        prompt.authenticate(promptInfo)
      }
    }
  }
}
