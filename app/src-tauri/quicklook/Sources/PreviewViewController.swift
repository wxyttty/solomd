// Quick Look preview for Markdown files: Finder's space-bar preview shows the
// note rendered, the way SoloMD's preview does, instead of plain text (#351).
//
// Data-based preview (macOS 12+): we hand Quick Look a self-contained HTML
// document and it draws it in its own sandboxed web view. Rendering happens
// here with markdown-it in JavaScriptCore; nothing is fetched from the network
// and raw HTML inside the note is escaped.
import Cocoa
import JavaScriptCore
import Quartz
import UniformTypeIdentifiers

@objc(PreviewViewController)
final class PreviewViewController: NSViewController, QLPreviewingController {
    override var nibName: NSNib.Name? { nil }
    override func loadView() { view = NSView() }

    func providePreview(for request: QLFilePreviewRequest) async throws -> QLPreviewReply {
        let url = request.fileURL
        let data = try Data(contentsOf: url)
        // Notes are UTF-8 almost always; fall back to Latin-1 so a legacy
        // encoding still previews instead of failing outright.
        let source = String(data: data, encoding: .utf8)
            ?? String(data: data, encoding: .isoLatin1) ?? ""
        let body = Self.render(source)
        let html = Self.page(title: url.deletingPathExtension().lastPathComponent, body: body)
        let reply = QLPreviewReply(dataOfContentType: .html, contentSize: CGSize(width: 820, height: 1000)) { _ in
            Data(html.utf8)
        }
        reply.stringEncoding = .utf8
        return reply
    }

    private static let renderer: JSContext? = {
        guard let ctx = JSContext(),
              let url = Bundle(for: PreviewViewController.self).url(forResource: "render", withExtension: "js"),
              let js = try? String(contentsOf: url, encoding: .utf8) else { return nil }
        ctx.evaluateScript(js)
        return ctx
    }()
    private static let lock = NSLock()

    static func render(_ markdown: String) -> String {
        lock.lock(); defer { lock.unlock() }
        guard let ctx = renderer,
              let fn = ctx.objectForKeyedSubscript("soloRender"),
              let out = fn.call(withArguments: [markdown]), out.isString,
              ctx.exception == nil else {
            // Renderer unavailable: show the text escaped rather than nothing.
            return "<pre>\(escape(markdown))</pre>"
        }
        return out.toString()
    }

    static func escape(_ s: String) -> String {
        s.replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
    }

    private static let css: String = {
        guard let url = Bundle(for: PreviewViewController.self).url(forResource: "preview", withExtension: "css"),
              let s = try? String(contentsOf: url, encoding: .utf8) else { return "" }
        return s
    }()

    static func page(title: String, body: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8">
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data: file:">
        <title>\(escape(title))</title><style>\(css)</style></head>
        <body><article class="markdown-body">\(body)</article></body></html>
        """
    }
}
