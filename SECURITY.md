# Security Policy

## Supported Versions

Security fixes are provided for the latest `1.0.x` release line:

| Version | Supported |
| --- | --- |
| `1.0.x` | Yes |
| `< 1.0.0` (including legacy **Shutter** releases) | No — please upgrade to the latest `1.0.x` release |

---

## Privacy & Security Architecture

Sky Shot is designed to keep your screenshots and extracted text local by default:

- **100% Offline OCR by Default**: Text extraction runs entirely on your device using native/local engines (`Windows.Media.Ocr` on Windows, Apple Vision on macOS, and bundled `ocrs` ONNX models on Linux). No screenshot is uploaded during capture or OCR.
- **Destructive Local Redaction**: Blur, pixelate, and solid-box redactions are burned directly into the exported pixels before an image is copied, saved, or indexed in History. Text under redacted areas is never indexed or stored in History. *(Note: blurred or pixelated text can sometimes be recovered by deblurring tools — use **Solid box** for passwords, tokens, and secrets.)*
- **No Cloud Uploads or Share Links**: Sky Shot has no upload backend or public link sharing. Captures stay strictly local (clipboard, disk, and local drag-and-drop shelf).
- **Optional BYO-Key AI Re-Read**: The "Re-read with AI" feature is strictly opt-in per capture. Only the cropped, already-redacted image is sent over HTTPS directly to the provider you configure (Anthropic Claude or Google Gemini). Your API key is never written to `settings.json` or exposed to the frontend webview — it is stored in your operating system's credential store (**Windows Credential Manager**, **macOS Keychain**, or **Linux Secret Service**).
- **Signed Auto-Updates & Scoped Local Protocols**: Release updates are cryptographically signed (minisign) and verified before installation. Internal `shot://` and `shelf://` protocols validate identifiers before reading local files to prevent path traversal.

---

## Reporting a Vulnerability

Please **do not** report security vulnerabilities through public GitHub issues.

Instead, report them privately using one of the following channels:

1. **GitHub Security Advisories (Preferred)**: Open a private advisory at
   [https://github.com/zahidhossin39/SkyShot/security/advisories/new](https://github.com/zahidhossin39/SkyShot/security/advisories/new).
2. **Direct Contact**: Reach out to project maintainer Masum Khan via [TheMasum.Com](https://themasum.com).

### What to Include

- Sky Shot version and operating system (e.g. `1.0.8`, Windows 11 / macOS / Linux).
- A clear description of the vulnerability and its potential impact.
- Step-by-step reproduction instructions or a minimal proof of concept.

We aim to acknowledge reports within **3 business days** and will coordinate with you on a fix and responsible disclosure timeline.
