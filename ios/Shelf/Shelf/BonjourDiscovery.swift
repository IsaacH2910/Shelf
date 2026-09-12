import Foundation
import Network

struct ShelfInstance: Identifiable, Equatable, Hashable {
    let id: String
    let name: String
    let advertisedURL: URL?
    let serviceType: String
    let domain: String

    var displayName: String {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Shelf" : trimmed
    }
}

@MainActor
final class BonjourDiscovery {
    private var browser: NWBrowser?
    private var resolver: NetServiceResolver?
    var onChange: ([ShelfInstance]) -> Void = { _ in }

    func start() {
        stop()
        let parameters = NWParameters()
        parameters.includePeerToPeer = false
        let descriptor = NWBrowser.Descriptor.bonjourWithTXTRecord(type: "_shelf._tcp", domain: "local.")
        let browser = NWBrowser(for: descriptor, using: parameters)
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            Task { @MainActor in
                self?.publish(results)
            }
        }
        browser.stateUpdateHandler = { state in
            if case .failed(let error) = state {
                NSLog("Shelf Bonjour browser failed: \(error)")
            }
        }
        browser.start(queue: .main)
        self.browser = browser
    }

    func stop() {
        browser?.cancel()
        browser = nil
        resolver?.cancel()
        resolver = nil
    }

    func resolve(_ instance: ShelfInstance) async -> URL? {
        if let advertised = instance.advertisedURL {
            return advertised
        }
        let resolver = NetServiceResolver()
        self.resolver = resolver
        let resolved = await resolver.resolve(
            name: instance.name,
            type: instance.serviceType.isEmpty ? "_shelf._tcp." : instance.serviceType,
            domain: instance.domain.isEmpty ? "local." : instance.domain
        )
        self.resolver = nil
        return resolved
    }

    private func publish(_ results: Set<NWBrowser.Result>) {
        let instances = results.compactMap(Self.instance(from:)).sorted {
            $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
        }
        onChange(instances)
    }

    private static func instance(from result: NWBrowser.Result) -> ShelfInstance? {
        guard case let .service(name: name, type: type, domain: domain, interface: _) = result.endpoint else {
            return nil
        }
        var advertisedURL: URL?
        if case let .bonjour(metadata) = result.metadata {
            advertisedURL = url(from: metadata.txtRecord)
        }
        return ShelfInstance(
            id: "\(name).\(type).\(domain)",
            name: name,
            advertisedURL: advertisedURL,
            serviceType: type,
            domain: domain
        )
    }

    private static func url(from record: NWTXTRecord) -> URL? {
        for key in ["url", "URL"] {
            if let value = record.dictionary[key], let parsed = URL(string: value), parsed.host != nil {
                return parsed
            }
        }
        return nil
    }
}

private final class NetServiceResolver: NSObject, NetServiceDelegate {
    private var service: NetService?
    private var continuation: CheckedContinuation<URL?, Never>?
    private var timeoutWork: DispatchWorkItem?

    func resolve(name: String, type: String, domain: String) async -> URL? {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            let service = NetService(domain: domain, type: type, name: name)
            service.delegate = self
            self.service = service
            service.resolve(withTimeout: 3)
            let work = DispatchWorkItem { [weak self] in
                self?.finish(nil)
            }
            timeoutWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 3.2, execute: work)
        }
    }

    func cancel() {
        finish(nil)
    }

    func netServiceDidResolveAddress(_ sender: NetService) {
        let host = (sender.hostName ?? "")
            .trimmingCharacters(in: CharacterSet(charactersIn: "."))
        let port = sender.port
        guard !host.isEmpty, port > 0 else {
            finish(nil)
            return
        }
        finish(URL(string: "http://\(host):\(port)/"))
    }

    func netService(_ sender: NetService, didNotResolve errorDict: [String: NSNumber]) {
        finish(nil)
    }

    private func finish(_ url: URL?) {
        timeoutWork?.cancel()
        timeoutWork = nil
        service?.stop()
        service = nil
        continuation?.resume(returning: url)
        continuation = nil
    }
}
