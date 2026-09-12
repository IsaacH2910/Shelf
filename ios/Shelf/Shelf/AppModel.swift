import Foundation
import Network
import Observation

enum ConnectionStatus: Equatable {
    case searching
    case connecting(String)
    case connected(url: URL, kind: ConnectionKind)
    case offline(String)

    var kind: ConnectionKind {
        switch self {
        case .connected(_, let kind):
            return kind
        case .offline, .searching, .connecting:
            return .offline
        }
    }

    var label: String {
        switch self {
        case .searching:
            return "Searching"
        case .connecting:
            return "Connecting"
        case .connected(_, .bonjour):
            return "Bonjour"
        case .connected(_, .lan):
            return "LAN"
        case .connected(_, .cloud):
            return "Cloud"
        case .connected(_, .offline), .offline:
            return "Offline"
        }
    }
}

@MainActor
@Observable
final class AppModel {
    var status: ConnectionStatus = .searching
    var instances: [ShelfInstance] = []
    var cloudHostname: String
    var manualLanText: String
    var lastError: String?
    var showsPicker = false
    var showsSettings = false

    private var preferences: Preferences
    private let discovery = BonjourDiscovery()
    private let pathMonitor = NWPathMonitor()
    private var healthTask: Task<Void, Never>?
    private var bootstrapTask: Task<Void, Never>?

    init(preferences: Preferences = Preferences()) {
        self.preferences = preferences
        self.cloudHostname = preferences.cloudHostname
        self.manualLanText = preferences.manualLanURL?.absoluteString ?? ""
    }

    var activeURL: URL? {
        if case let .connected(url, _) = status {
            return url
        }
        return nil
    }

    func start() {
        discovery.onChange = { [weak self] instances in
            self?.instances = instances
        }
        discovery.start()
        pathMonitor.pathUpdateHandler = { [weak self] path in
            Task { @MainActor in
                guard let self else { return }
                if path.status == .satisfied {
                    await self.reconnectIfNeeded()
                } else if case .connected = self.status {
                    self.status = .offline("This device is offline.")
                }
            }
        }
        pathMonitor.start(queue: .main)
        bootstrapTask?.cancel()
        bootstrapTask = Task { await bootstrap() }
        startHealthLoop()
    }

    func stop() {
        bootstrapTask?.cancel()
        healthTask?.cancel()
        discovery.stop()
        pathMonitor.cancel()
    }

    func saveCloudHostname(_ raw: String) {
        preferences.cloudHostname = Preferences.normalizedHost(raw)
        cloudHostname = preferences.cloudHostname
    }

    func saveManualLanURL(_ raw: String) {
        preferences.manualLanURL = Preferences.parseURL(raw)
        manualLanText = preferences.manualLanURL?.absoluteString ?? raw
    }

    func findMac() async {
        await bootstrap()
    }

    func connect(to instance: ShelfInstance) async {
        status = .connecting(instance.displayName)
        lastError = nil
        if let url = await discovery.resolve(instance), await OriginProbe.isHealthy(url) {
            preferences.lastLocalURL = url
            open(url, kind: .bonjour)
            return
        }
        lastError = "Found \(instance.displayName), but it did not answer /api/health."
        await continueAfterBonjour()
    }

    func connect(to url: URL, kind: ConnectionKind) async {
        status = .connecting(url.host ?? url.absoluteString)
        lastError = nil
        if await OriginProbe.isHealthy(url) {
            if kind == .lan || kind == .bonjour {
                preferences.lastLocalURL = url
            }
            open(url, kind: kind)
            return
        }
        lastError = "Could not reach \(url.absoluteString)"
        if kind == .bonjour || kind == .lan {
            await fallbackToCloud(reason: lastError)
        } else {
            status = .offline(lastError ?? "Offline")
        }
    }

    func useCloud() async {
        guard let url = preferences.cloudURL else {
            showsSettings = true
            lastError = "Set a Cloudflare hostname in Settings."
            status = .offline(lastError ?? "Offline")
            return
        }
        await connect(to: url, kind: .cloud)
    }

    func reconnectIfNeeded() async {
        if case .connected(let url, _) = status, await OriginProbe.isHealthy(url) {
            return
        }
        await bootstrap()
    }

    /// 1. Bonjour  2. remembered / typed LAN URL  3. Cloudflare
    private func bootstrap() async {
        status = .searching
        lastError = nil

        try? await Task.sleep(for: .milliseconds(1400))
        if Task.isCancelled { return }

        if let first = instances.first {
            status = .connecting(first.displayName)
            if let url = await discovery.resolve(first), await OriginProbe.isHealthy(url) {
                preferences.lastLocalURL = url
                open(url, kind: .bonjour)
                return
            }
            lastError = "A Bonjour Shelf appeared but did not answer /api/health."
        }

        await continueAfterBonjour()
    }

    private func continueAfterBonjour() async {
        if Task.isCancelled { return }
        for url in preferences.lanCandidates {
            if await OriginProbe.isHealthy(url) {
                open(url, kind: .lan)
                return
            }
        }
        await fallbackToCloud(reason: lastError ?? (instances.isEmpty
            ? "No Mac in Connect iPhone / iPad mode on this Wi-Fi."
            : lastError))
    }

    private func fallbackToCloud(reason: String?) async {
        if let url = preferences.cloudURL, await OriginProbe.isHealthy(url) {
            lastError = reason
            open(url, kind: .cloud)
            return
        }
        showsPicker = true
        status = .offline(reason ?? "Bonjour missed, LAN URL missed, and Cloudflare is not reachable.")
    }

    private func open(_ url: URL, kind: ConnectionKind) {
        preferences.lastKind = kind
        status = .connected(url: url, kind: kind)
        showsPicker = false
    }

    private func startHealthLoop() {
        healthTask?.cancel()
        healthTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(12))
                guard let self, !Task.isCancelled else { return }
                await self.reconnectIfNeeded()
            }
        }
    }
}
