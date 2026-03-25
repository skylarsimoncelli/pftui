import CryptoKit
import Foundation
import Security

@MainActor
final class MobileStore: ObservableObject {
    @Published var connection: ConnectionSettings?
    @Published var dashboard: DashboardPayload?
    @Published var portfolio: PortfolioPayload?
    @Published var analytics: AnalyticsPayload?
    @Published var errorMessage: String?
    @Published var isBusy = false

    private let connectionKey = "pftui.mobile.connection"
    private let tokenAccount = "pftui-mobile-token"

    init() {
        loadPersisted()
    }

    func connect(server: String, apiToken: String, fingerprint: String) async {
        let settings = ConnectionSettings(
            server: normalizeServer(server),
            fingerprint: fingerprint.trimmingCharacters(in: .whitespacesAndNewlines),
            token: apiToken.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        guard !settings.server.isEmpty, !settings.token.isEmpty, !settings.fingerprint.isEmpty else {
            errorMessage = "Enter the server, API token, and TLS fingerprint."
            return
        }

        isBusy = true
        defer { isBusy = false }

        do {
            let payload: DashboardPayload = try await request(path: "/api/dashboard", connection: settings)
            dashboard = payload
            portfolio = payload.portfolio
            analytics = payload.analytics
            connection = settings
            saveConnection(settings)
            KeychainHelper.save(password: settings.token, account: tokenAccount)
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func reconnectIfPossible() async {
        guard var connection else { return }
        guard let token = KeychainHelper.loadPassword(account: tokenAccount) else { return }
        connection.token = token
        self.connection = connection
        await refresh()
    }

    func disconnect() {
        connection = nil
        dashboard = nil
        portfolio = nil
        analytics = nil
        errorMessage = nil
        UserDefaults.standard.removeObject(forKey: connectionKey)
        KeychainHelper.deletePassword(account: tokenAccount)
    }

    func refresh() async {
        guard let connection, !connection.token.isEmpty else {
            await reconnectIfPossible()
            return
        }
        isBusy = true
        defer { isBusy = false }

        do {
            let payload: DashboardPayload = try await request(path: "/api/dashboard", connection: connection)
            dashboard = payload
            portfolio = payload.portfolio
            analytics = payload.analytics
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func request<T: Decodable>(path: String, connection: ConnectionSettings? = nil) async throws -> T {
        let activeConnection = connection ?? self.connection
        guard let activeConnection else { throw APIError.missingConnection }
        guard let url = URL(string: "https://\(activeConnection.server)\(path)") else {
            throw APIError.invalidURL
        }

        let delegate = PinnedSessionDelegate(fingerprint: normalizeFingerprint(activeConnection.fingerprint))
        let session = URLSession(configuration: .ephemeral, delegate: delegate, delegateQueue: nil)
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(activeConnection.token)", forHTTPHeaderField: "Authorization")

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch let urlError as URLError where urlError.code == .cancelled {
            throw delegate.lastError ?? APIError.tlsTrustFailed
        } catch {
            throw error
        }
        guard let http = response as? HTTPURLResponse else { throw APIError.invalidResponse }
        guard (200..<300).contains(http.statusCode) else {
            let message = String(data: data, encoding: .utf8) ?? "Unknown server error"
            throw APIError.server(message)
        }

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(T.self, from: data)
    }

    private func loadPersisted() {
        if let data = UserDefaults.standard.data(forKey: connectionKey),
           let settings = try? JSONDecoder().decode(PersistedConnection.self, from: data) {
            let token = KeychainHelper.loadPassword(account: tokenAccount) ?? ""
            connection = ConnectionSettings(server: settings.server, fingerprint: settings.fingerprint, token: token)
        }
    }

    private func saveConnection(_ settings: ConnectionSettings) {
        let persisted = PersistedConnection(server: settings.server, fingerprint: settings.fingerprint)
        if let data = try? JSONEncoder().encode(persisted) {
            UserDefaults.standard.set(data, forKey: connectionKey)
        }
    }

    fileprivate func normalizeFingerprint(_ value: String) -> String {
        value.uppercased().replacingOccurrences(of: "[^A-F0-9]", with: "", options: .regularExpression)
    }

    private func normalizeServer(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "" }

        let candidate = trimmed.contains("://") ? trimmed : "https://\(trimmed)"
        if let components = URLComponents(string: candidate), let host = components.host {
            let normalizedHost: String
            if host.contains(":") {
                normalizedHost = "[\(host)]"
            } else {
                normalizedHost = host
            }
            let port = components.port ?? 9443
            return "\(normalizedHost):\(port)"
        }

        let stripped = trimmed
            .replacingOccurrences(of: #"^https?://"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        if stripped.contains(":") {
            return stripped
        }
        return "\(stripped):9443"
    }
}

private struct PersistedConnection: Codable {
    let server: String
    let fingerprint: String
}

enum APIError: LocalizedError {
    case missingConnection
    case invalidURL
    case invalidResponse
    case server(String)
    case tlsPinMismatch
    case tlsTrustFailed

    var errorDescription: String? {
        switch self {
        case .missingConnection: return "Missing mobile server connection details."
        case .invalidURL: return "The mobile server URL is invalid."
        case .invalidResponse: return "The server returned an invalid response."
        case .server(let message): return message
        case .tlsPinMismatch: return "TLS fingerprint mismatch. Verify the server fingerprint and try again."
        case .tlsTrustFailed: return "TLS trust evaluation failed for the pinned certificate."
        }
    }
}

final class PinnedSessionDelegate: NSObject, URLSessionDelegate, URLSessionTaskDelegate {
    private let fingerprint: String
    private(set) var lastError: APIError?

    init(fingerprint: String) {
        self.fingerprint = fingerprint
    }

    private func handleChallenge(
        _ challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust else {
            completionHandler(.performDefaultHandling, nil)
            return
        }
        guard let trust = challenge.protectionSpace.serverTrust,
              let certificate = (SecTrustCopyCertificateChain(trust) as? [SecCertificate])?.first else {
            lastError = .tlsTrustFailed
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }

        let data = SecCertificateCopyData(certificate) as Data
        let digest = SHA256.hash(data: data)
        let observed = digest.map { String(format: "%02X", $0) }.joined()
        guard observed == fingerprint else {
            lastError = .tlsPinMismatch
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }

        // Trust the pinned self-signed leaf as the only anchor for this session.
        let anchors: CFArray = [certificate] as CFArray
        SecTrustSetAnchorCertificates(trust, anchors)
        SecTrustSetAnchorCertificatesOnly(trust, true)
        guard SecTrustEvaluateWithError(trust, nil) else {
            lastError = .tlsTrustFailed
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }

        completionHandler(.useCredential, URLCredential(trust: trust))
    }

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        handleChallenge(challenge, completionHandler: completionHandler)
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        handleChallenge(challenge, completionHandler: completionHandler)
    }
}

enum KeychainHelper {
    static func save(password: String, account: String) {
        let data = Data(password.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: account,
            kSecValueData as String: data,
        ]
        SecItemDelete(query as CFDictionary)
        SecItemAdd(query as CFDictionary, nil)
    }

    static func loadPassword(account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess,
              let data = item as? Data,
              let password = String(data: data, encoding: .utf8) else {
            return nil
        }
        return password
    }

    static func deletePassword(account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
