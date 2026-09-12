import Foundation

enum OriginProbe {
    struct Health: Decodable {
        let status: String
        let loginReady: Bool?
        let lan: Bool?
        let localUrl: String?
    }

    static func isHealthy(_ base: URL, timeout: TimeInterval = 2) async -> Bool {
        await health(base, timeout: timeout) != nil
    }

    static func health(_ base: URL, timeout: TimeInterval = 2) async -> Health? {
        guard let url = healthURL(from: base) else { return nil }
        var request = URLRequest(url: url)
        request.timeoutInterval = timeout
        request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                return nil
            }
            return try JSONDecoder().decode(Health.self, from: data)
        } catch {
            return nil
        }
    }

    static func healthURL(from base: URL) -> URL? {
        var components = URLComponents(url: base, resolvingAgainstBaseURL: false)
        components?.path = "/api/health"
        components?.query = nil
        components?.fragment = nil
        return components?.url
    }
}
