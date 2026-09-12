import Foundation

struct Preferences {
    private enum Key {
        static let lastLocalURL = "shelf.lastLocalURL"
        static let manualLanURL = "shelf.manualLanURL"
        static let cloudHostname = "shelf.cloudHostname"
        static let lastKind = "shelf.lastKind"
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    var lastLocalURL: URL? {
        get { defaults.string(forKey: Key.lastLocalURL).flatMap(URL.init(string:)) }
        set { defaults.set(newValue?.absoluteString, forKey: Key.lastLocalURL) }
    }

    var manualLanURL: URL? {
        get { defaults.string(forKey: Key.manualLanURL).flatMap(Self.parseURL) }
        set { defaults.set(newValue?.absoluteString, forKey: Key.manualLanURL) }
    }

    var lanCandidates: [URL] {
        [manualLanURL, lastLocalURL].compactMap { $0 }
    }

    static func parseURL(_ raw: String) -> URL? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if let url = URL(string: trimmed), url.scheme != nil, url.host != nil {
            return url
        }
        return URL(string: "http://\(trimmed)")
    }

    var cloudHostname: String {
        get { defaults.string(forKey: Key.cloudHostname) ?? "" }
        set { defaults.set(newValue.trimmingCharacters(in: .whitespacesAndNewlines), forKey: Key.cloudHostname) }
    }

    var cloudURL: URL? {
        let host = Self.normalizedHost(cloudHostname)
        guard !host.isEmpty else { return nil }
        return URL(string: "https://\(host)")
    }

    var lastKind: ConnectionKind? {
        get { defaults.string(forKey: Key.lastKind).flatMap(ConnectionKind.init(rawValue:)) }
        set { defaults.set(newValue?.rawValue, forKey: Key.lastKind) }
    }

    static func normalizedHost(_ raw: String) -> String {
        var host = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        host = host.replacingOccurrences(of: "https://", with: "", options: .caseInsensitive)
        host = host.replacingOccurrences(of: "http://", with: "", options: .caseInsensitive)
        if let slash = host.firstIndex(of: "/") {
            host = String(host[..<slash])
        }
        if let colon = host.firstIndex(of: ":"), !host.contains("]") {
            host = String(host[..<colon])
        }
        return host.trimmingCharacters(in: CharacterSet(charactersIn: "."))
            .lowercased()
    }
}

enum ConnectionKind: String, Equatable {
    case bonjour
    case lan
    case cloud
    case offline
}
