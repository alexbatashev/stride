import SwiftUI
import WebKit

/// Renders a self-contained interactive HTML artifact inside a hardened,
/// sandboxed web view — the native counterpart to the web client's sandboxed
/// iframe. The document runs with no network access and no bridge back to the
/// app beyond reporting its content height.
struct ArtifactWebView: View {
    let html: String
    @State private var height: CGFloat = 220

    var body: some View {
        ArtifactWebViewRepresentable(document: ArtifactDocument.wrap(html), height: $height)
            .frame(height: height)
            .frame(maxWidth: .infinity)
            .background(Color.subtleFill, in: .rect(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.hairline))
    }
}

/// Shown while an artifact is still streaming and has no closing fence yet.
struct ArtifactPlaceholder: View {
    var body: some View {
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text("Building interactive view…")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(Color.subtleFill, in: .rect(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.hairline, style: StrokeStyle(lineWidth: 1, dash: [4])))
    }
}

/// Builds the document loaded into the sandbox. The Content-Security-Policy
/// matches the web client: no network egress (`connect-src 'none'` blocks
/// exfiltration and external loads), inline script/style only, images limited to
/// inline data. A small bridge reports content height to the host.
enum ArtifactDocument {
    static let maxBytes = 256 * 1024

    private static let csp = [
        "default-src 'none'",
        "script-src 'unsafe-inline'",
        "style-src 'unsafe-inline'",
        "img-src data: blob:",
        "font-src data:",
        "connect-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "frame-src 'none'",
    ].joined(separator: "; ")

    private static let heightBridge = """
    (function () {
      function report() {
        var h = document.documentElement.scrollHeight;
        if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.strideArtifact) {
          window.webkit.messageHandlers.strideArtifact.postMessage(h);
        }
      }
      if (window.ResizeObserver) { new ResizeObserver(report).observe(document.documentElement); }
      window.addEventListener("load", report);
      report();
    })();
    """

    static func wrap(_ html: String) -> String {
        let body = html.utf8.count > maxBytes ? "<p>This interactive view is too large to display.</p>" : html
        return """
        <!doctype html><html><head><meta charset="utf-8">
        <meta http-equiv="Content-Security-Policy" content="\(csp)">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>html,body{margin:0;padding:0;}body{font-family:-apple-system,system-ui,sans-serif;font-size:16px;}</style>
        </head><body>\(body)<script>\(heightBridge)</script></body></html>
        """
    }
}

#if canImport(UIKit)
private struct ArtifactWebViewRepresentable: UIViewRepresentable {
    let document: String
    @Binding var height: CGFloat

    func makeCoordinator() -> ArtifactCoordinator { ArtifactCoordinator(height: $height) }
    func makeUIView(context: Context) -> WKWebView { context.coordinator.makeWebView() }
    func updateUIView(_ webView: WKWebView, context: Context) { context.coordinator.load(document, into: webView) }
}
#elseif canImport(AppKit)
private struct ArtifactWebViewRepresentable: NSViewRepresentable {
    let document: String
    @Binding var height: CGFloat

    func makeCoordinator() -> ArtifactCoordinator { ArtifactCoordinator(height: $height) }
    func makeNSView(context: Context) -> WKWebView { context.coordinator.makeWebView() }
    func updateNSView(_ webView: WKWebView, context: Context) { context.coordinator.load(document, into: webView) }
}
#endif

/// Owns the web view's hardened configuration and the single height channel back
/// to SwiftUI. No other native bridge is exposed, and navigation away from the
/// loaded document is cancelled.
private final class ArtifactCoordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
    private let height: Binding<CGFloat>
    private var loaded: String?

    init(height: Binding<CGFloat>) { self.height = height }

    func makeWebView() -> WKWebView {
        let controller = WKUserContentController()
        controller.add(self, name: "strideArtifact")

        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.userContentController = controller
        config.defaultWebpagePreferences.allowsContentJavaScript = true

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = self
        webView.allowsLinkPreview = false
        #if canImport(UIKit)
        webView.scrollView.isScrollEnabled = false
        webView.isOpaque = false
        webView.backgroundColor = .clear
        #endif
        return webView
    }

    func load(_ document: String, into webView: WKWebView) {
        guard loaded != document else { return }
        loaded = document
        webView.loadHTMLString(document, baseURL: nil)
    }

    // Allow only the initial in-memory load; cancel any attempt to navigate away.
    func webView(
        _ webView: WKWebView,
        decidePolicyFor navigationAction: WKNavigationAction,
        decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
    ) {
        let url = navigationAction.request.url
        if url == nil || url?.scheme == "about" {
            decisionHandler(.allow)
        } else {
            decisionHandler(.cancel)
        }
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard message.name == "strideArtifact", let value = message.body as? NSNumber else { return }
        let clamped = min(max(CGFloat(value.doubleValue), 60), 4000)
        DispatchQueue.main.async { self.height.wrappedValue = clamped }
    }
}
