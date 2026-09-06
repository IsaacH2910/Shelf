import Darwin
import Foundation
import NaturalLanguage
import Translation

@_cdecl("shelf_apple_translate_free")
public func shelf_apple_translate_free(_ ptr: UnsafeMutablePointer<CChar>?) {
    if let ptr {
        free(ptr)
    }
}

@_cdecl("shelf_apple_translate")
public func shelf_apple_translate(
    _ text: UnsafePointer<CChar>?,
    _ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let text, let out else {
        return -1
    }
    out.pointee = nil
    let source = String(cString: text)
    let trimmed = source.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty {
        return -2
    }

    if #available(macOS 26.0, *) {
        switch translateInstalled(trimmed) {
        case .success(let translated):
            return strdupOut(translated, out)
        case .failure:
            return -4
        }
    }
    return -4
}

private func strdupOut(_ value: String, _ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>) -> Int32 {
    value.withCString { cstr in
        guard let copy = strdup(cstr) else {
            return -3
        }
        out.pointee = copy
        return 0
    }
}

private func sourceLanguage(for text: String) -> String? {
    let recognizer = NLLanguageRecognizer()
    recognizer.processString(text)
    switch recognizer.dominantLanguage {
    case .japanese: return "ja"
    case .korean: return "ko"
    case .english: return "en"
    default: return nil
    }
}

@available(macOS 26.0, *)
private func translateInstalled(_ text: String) -> Result<String, Error> {
    guard let sourceId = sourceLanguage(for: text) else {
        return .failure(AppleTranslateFailure.unsupportedSource)
    }

    let source = Locale.Language(identifier: sourceId)
    let target = Locale.Language(identifier: "zh-Hant")
    let box = ResultBox()
    let lock = DispatchSemaphore(value: 0)

    Task { @MainActor in
        do {
            let session = TranslationSession(installedSource: source, target: target)
            if await session.isReady == false {
                try await session.prepareTranslation()
            }
            let response = try await session.translate(text)
            box.result = .success(response.targetText)
        } catch {
            box.result = .failure(error)
        }
        lock.signal()
    }

    if lock.wait(timeout: .now() + 25) == .timedOut {
        return .failure(AppleTranslateFailure.timeout)
    }
    return box.result ?? .failure(AppleTranslateFailure.timeout)
}

private enum AppleTranslateFailure: Error {
    case unsupportedSource
    case timeout
}

private final class ResultBox: @unchecked Sendable {
    var result: Result<String, Error>?
}
