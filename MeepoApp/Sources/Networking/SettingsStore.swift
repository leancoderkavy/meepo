import Foundation
import SwiftUI

@MainActor
final class SettingsStore: ObservableObject {
    @AppStorage("meepo_host") var host: String = "127.0.0.1"
    @AppStorage("meepo_port") var port: Int = 18789
    @AppStorage("meepo_auth_token") var authToken: String = ""
    @AppStorage("meepo_use_tls") var useTLS: Bool = false
    @AppStorage("meepo_haptics") var hapticsEnabled: Bool = true

    var gatewayDisplayURL: String {
        let scheme = useTLS ? "wss" : "ws"
        return "\(scheme)://\(host):\(port)/ws"
    }

    var isConfigured: Bool {
        !host.isEmpty && port > 0
    }
}
