import SwiftUI

struct SettingsView: View {
    @ObservedObject var settings: SettingsStore
    @ObservedObject var client: MeepoClient
    @State private var statusResponse: StatusResponse?
    @State private var statusError: String?
    @State private var isTestingConnection = false

    var body: some View {
        ScrollView {
            VStack(spacing: 16) {
                // ── Connection Card ──
                SettingsCard {
                    SettingsCardHeader(icon: "network", title: "Connection")

                    MeepoTextField(label: "Host", text: $settings.host, placeholder: "127.0.0.1")
                        .textContentType(.URL)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)

                    MeepoDivider()

                    HStack {
                        Text("Port")
                            .foregroundStyle(MeepoTheme.parchment)
                        Spacer()
                        TextField("18789", value: $settings.port, format: .number)
                            .multilineTextAlignment(.trailing)
                            .keyboardType(.numberPad)
                            .foregroundStyle(MeepoTheme.warmTan)
                    }

                    MeepoDivider()

                    HStack {
                        Text("Use TLS")
                            .foregroundStyle(MeepoTheme.parchment)
                        Spacer()
                        Toggle("", isOn: $settings.useTLS)
                            .labelsHidden()
                            .tint(MeepoTheme.hoodBlue)
                    }

                    MeepoDivider()

                    HStack {
                        Text("Gateway URL")
                            .foregroundStyle(MeepoTheme.dusty)
                            .font(.caption)
                        Spacer()
                        Text(settings.gatewayDisplayURL)
                            .font(.caption.monospaced())
                            .foregroundStyle(MeepoTheme.warmTan)
                            .textSelection(.enabled)
                    }
                }

                // ── Authentication Card ──
                SettingsCard {
                    SettingsCardHeader(icon: "key.fill", title: "Authentication")

                    SecureField("Gateway Token", text: $settings.authToken)
                        .textContentType(.password)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .foregroundStyle(MeepoTheme.parchment)
                        .padding(10)
                        .background(MeepoTheme.caveDark)
                        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

                    Text("Set MEEPO_GATEWAY_TOKEN in your Meepo config, then enter the same value here.")
                        .font(.caption)
                        .foregroundStyle(MeepoTheme.shadow)
                }

                // ── Status Card ──
                SettingsCard {
                    SettingsCardHeader(icon: "heart.text.square", title: "Status")

                    Button {
                        testConnection()
                    } label: {
                        HStack {
                            Label("Test Connection", systemImage: "bolt.horizontal")
                                .foregroundStyle(MeepoTheme.parchment)
                            Spacer()
                            if isTestingConnection {
                                ProgressView()
                                    .controlSize(.small)
                                    .tint(MeepoTheme.warmTan)
                            } else {
                                connectionStatusIcon
                            }
                        }
                        .padding(12)
                        .background(MeepoTheme.caveDark)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }
                    .disabled(isTestingConnection)

                    if let status = statusResponse {
                        VStack(spacing: 0) {
                            StatusRow(label: "Status", value: status.status, color: MeepoTheme.gemGreen)
                            MeepoDivider()
                            StatusRow(label: "Uptime", value: status.formattedUptime, color: MeepoTheme.warmTan)
                            MeepoDivider()
                            StatusRow(label: "Sessions", value: "\(status.sessions)", color: MeepoTheme.hoodBlue)
                            MeepoDivider()
                            StatusRow(label: "Clients", value: "\(status.connectedClients)", color: MeepoTheme.hoodBlue)
                        }
                        .padding(12)
                        .background(MeepoTheme.caveDark)
                        .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    }

                    if let error = statusError {
                        HStack(spacing: 6) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(MeepoTheme.bloodRed)
                            Text(error)
                                .font(.caption)
                                .foregroundStyle(MeepoTheme.bloodRed)
                        }
                    }
                }

                // ── Preferences Card ──
                SettingsCard {
                    SettingsCardHeader(icon: "slider.horizontal.3", title: "Preferences")

                    HStack {
                        Text("Haptic Feedback")
                            .foregroundStyle(MeepoTheme.parchment)
                        Spacer()
                        Toggle("", isOn: $settings.hapticsEnabled)
                            .labelsHidden()
                            .tint(MeepoTheme.hoodBlue)
                    }
                }

                // ── About Card ──
                SettingsCard {
                    SettingsCardHeader(icon: "info.circle", title: "About Meepo")

                    HStack {
                        MeepoAvatar(size: 48)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Meepo")
                                .font(.headline)
                                .foregroundStyle(MeepoTheme.parchment)
                            Text("\"If you ask me, life is all about who you know and what you can find.\"")
                                .font(.caption.italic())
                                .foregroundStyle(MeepoTheme.dusty)
                        }
                    }

                    MeepoDivider()

                    HStack {
                        Text("Version")
                            .foregroundStyle(MeepoTheme.dusty)
                        Spacer()
                        Text("1.0.0")
                            .foregroundStyle(MeepoTheme.warmTan)
                    }

                    HStack {
                        Text("Protocol")
                            .foregroundStyle(MeepoTheme.dusty)
                        Spacer()
                        Text("Gateway WS v1")
                            .foregroundStyle(MeepoTheme.warmTan)
                    }

                    MeepoDivider()

                    Link(destination: URL(string: "https://github.com/kavyrattana/meepo")!) {
                        HStack {
                            Image(systemName: "link")
                                .foregroundStyle(MeepoTheme.hoodBlue)
                            Text("GitHub Repository")
                                .foregroundStyle(MeepoTheme.parchment)
                            Spacer()
                            Image(systemName: "arrow.up.right")
                                .font(.caption)
                                .foregroundStyle(MeepoTheme.shadow)
                        }
                    }

                    Link(destination: URL(string: "https://dota2.fandom.com/wiki/Meepo/Lore")!) {
                        HStack {
                            Image(systemName: "book")
                                .foregroundStyle(MeepoTheme.earthBrown)
                            Text("Meepo Lore")
                                .foregroundStyle(MeepoTheme.parchment)
                            Spacer()
                            Image(systemName: "arrow.up.right")
                                .font(.caption)
                                .foregroundStyle(MeepoTheme.shadow)
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
        }
        .background(MeepoTheme.caveDark)
        .navigationTitle("Settings")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(MeepoTheme.barBackground, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
    }

    @ViewBuilder
    private var connectionStatusIcon: some View {
        switch client.connectionState {
        case .connected:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(MeepoTheme.gemGreen)
        case .error:
            Image(systemName: "xmark.circle.fill")
                .foregroundStyle(MeepoTheme.bloodRed)
        default:
            Image(systemName: "circle")
                .foregroundStyle(MeepoTheme.shadow)
        }
    }

    private func testConnection() {
        isTestingConnection = true
        statusResponse = nil
        statusError = nil

        Task {
            do {
                statusResponse = try await client.fetchStatus()
                statusError = nil
            } catch {
                statusError = error.localizedDescription
                statusResponse = nil
            }
            isTestingConnection = false
        }
    }
}

// MARK: - Settings Card Components

struct SettingsCard<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            content
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MeepoTheme.caveStone)
        .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.cardRadius, style: .continuous))
    }
}

struct SettingsCardHeader: View {
    let icon: String
    let title: String

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .font(.caption.bold())
                .foregroundStyle(MeepoTheme.goldAccent)
                .frame(width: 20)
            Text(title)
                .font(.subheadline.bold())
                .foregroundStyle(MeepoTheme.warmTan)
        }
    }
}

struct MeepoTextField: View {
    let label: String
    @Binding var text: String
    var placeholder: String = ""

    var body: some View {
        HStack {
            Text(label)
                .foregroundStyle(MeepoTheme.parchment)
            Spacer()
            TextField(placeholder, text: $text)
                .multilineTextAlignment(.trailing)
                .foregroundStyle(MeepoTheme.warmTan)
        }
    }
}

struct MeepoDivider: View {
    var body: some View {
        Rectangle()
            .fill(MeepoTheme.divider)
            .frame(height: 0.5)
    }
}

struct StatusRow: View {
    let label: String
    let value: String
    var color: Color = MeepoTheme.warmTan

    var body: some View {
        HStack {
            Text(label)
                .font(.subheadline)
                .foregroundStyle(MeepoTheme.dusty)
            Spacer()
            Text(value)
                .font(.subheadline.monospaced())
                .foregroundStyle(color)
        }
        .padding(.vertical, 4)
    }
}
