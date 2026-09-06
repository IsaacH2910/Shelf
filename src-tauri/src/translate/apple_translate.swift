import Foundation
import Translation

@_cdecl("shelf_apple_translate")
public func shelf_apple_translate(_ text: UnsafePointer<CChar>, _ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32 {
    out.pointee = nil
    let input = String(cString: text)
    if input.isEmpty { return -1 }

    if #available(macOS 15.0, *) {
        let semaphore = DispatchSemaphore(value: 0)
        var result: String?
        var failed = false
        Task { @MainActor in
            do {
                let source = Locale.Language(identifier: detectSource(input))
                let target = Locale.Language(identifier: "zh-Hant")
                var configuration = TranslationSession.Configuration(source: source, target: target)
                // Headless TranslationSession is not publicly constructible without a view
                // in current SDKs; mark unavailable so Rust falls back.
                _ = configuration
                failed = true
            }
            semaphore.signal()
        }
        _ = semaphore.wait(timeout: .now() + 8)
        if let result, !result.isEmpty {
            result.withCString { cstr in
                out.pointee = strdup(cstr)
            }
            return 0
        }
        _ = failed
    }
    return -4
}

@_cdecl("shelf_apple_translate_free")
public func shelf_apple_translate_free(_ ptr: UnsafeMutablePointer<CChar>?) {
    if let ptr { free(ptr) }
}

func detectSource(_ text: String) -> String {
    if text.unicodeScalars.contains(where: { (0x3040...0x30FF).contains($0.value) }) { return "ja" }
    if text.unicodeScalars.contains(where: { (0xAC00...0xD7AF).contains($0.value) }) { return "ko" }
    return "en"
}
