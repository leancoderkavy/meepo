import SwiftUI

struct SessionsView: View {
    @ObservedObject var viewModel: ChatViewModel
    @ObservedObject var client: MeepoClient
    @State private var showNewSession = false
    @State private var newSessionName = ""
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Header card
                VStack(spacing: 8) {
                    HStack {
                        MeepoAvatar(size: 40)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Tunnels")
                                .font(.title3.bold())
                                .foregroundStyle(MeepoTheme.parchment)
                            Text(client.connectionState.isConnected
                                 ? "\(viewModel.sessions.count) active"
                                 : "Disconnected")
                                .font(.caption)
                                .foregroundStyle(MeepoTheme.dusty)
                        }
                        Spacer()
                        Button {
                            showNewSession = true
                        } label: {
                            Image(systemName: "plus.circle.fill")
                                .font(.title3)
                                .foregroundStyle(MeepoTheme.goldAccent)
                        }
                        .disabled(!client.connectionState.isConnected)
                    }
                }
                .padding(16)
                .background(MeepoTheme.caveStone)
                .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.cardRadius, style: .continuous))
                .padding(.horizontal, 16)
                .padding(.top, 12)

                // Session list
                if viewModel.sessions.isEmpty && client.connectionState.isConnected {
                    VStack(spacing: 12) {
                        Text("⛏")
                            .font(.system(size: 32))
                        Text("No tunnels yet")
                            .font(.subheadline)
                            .foregroundStyle(MeepoTheme.dusty)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 48)
                } else {
                    LazyVStack(spacing: 2) {
                        ForEach(viewModel.sessions) { session in
                            Button {
                                viewModel.switchSession(to: session.id)
                                dismiss()
                            } label: {
                                SessionRow(
                                    session: session,
                                    isActive: session.id == viewModel.currentSessionId
                                )
                            }
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 12)
                }

                if !client.connectionState.isConnected {
                    HStack(spacing: 8) {
                        Image(systemName: "wifi.slash")
                            .foregroundStyle(MeepoTheme.bloodRed)
                        Text("Connect to Meepo to see your tunnels")
                            .font(.subheadline)
                            .foregroundStyle(MeepoTheme.dusty)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(20)
                    .background(MeepoTheme.caveStone)
                    .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.cardRadius, style: .continuous))
                    .padding(.horizontal, 16)
                    .padding(.top, 16)
                }
            }
            .padding(.bottom, 20)
        }
        .background(MeepoTheme.caveDark)
        .navigationTitle("Sessions")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(MeepoTheme.barBackground, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .onAppear {
            if client.connectionState.isConnected {
                viewModel.loadSessions()
            }
        }
        .alert("New Tunnel", isPresented: $showNewSession) {
            TextField("Session name", text: $newSessionName)
            Button("Dig") {
                let name = newSessionName.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !name.isEmpty else { return }
                viewModel.createSession(name: name)
                newSessionName = ""
            }
            Button("Cancel", role: .cancel) {
                newSessionName = ""
            }
        }
    }
}

// MARK: - Session Row

struct SessionRow: View {
    let session: MeepoSession
    let isActive: Bool

    var body: some View {
        HStack(spacing: 12) {
            // Session kind icon
            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(isActive ? MeepoTheme.hoodBlue.opacity(0.2) : MeepoTheme.caveWall)
                    .frame(width: 36, height: 36)
                Image(systemName: sessionIcon)
                    .font(.caption.bold())
                    .foregroundStyle(isActive ? MeepoTheme.hoodBlue : MeepoTheme.dusty)
            }

            VStack(alignment: .leading, spacing: 3) {
                Text(session.displayName)
                    .font(.body.weight(isActive ? .semibold : .regular))
                    .foregroundStyle(MeepoTheme.parchment)

                HStack(spacing: 8) {
                    if let kind = session.kind {
                        Text(kind.capitalized)
                            .font(.caption2.weight(.medium))
                            .foregroundStyle(MeepoTheme.warmTan)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(MeepoTheme.earthBrown.opacity(0.2))
                            .clipShape(Capsule())
                    }

                    if let count = session.messageCount, count > 0 {
                        Text("\(count) msgs")
                            .font(.caption2)
                            .foregroundStyle(MeepoTheme.dusty)
                    }

                    if let activity = session.formattedLastActivity {
                        Text(activity)
                            .font(.caption2)
                            .foregroundStyle(MeepoTheme.shadow)
                    }
                }
            }

            Spacer()

            if isActive {
                Image(systemName: "checkmark.circle.fill")
                    .font(.body)
                    .foregroundStyle(MeepoTheme.hoodBlue)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(isActive ? MeepoTheme.caveStone : Color.clear)
        .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.cardRadius, style: .continuous))
    }

    private var sessionIcon: String {
        switch session.kind?.lowercased() {
        case "main": return "house"
        case "group": return "person.3"
        case "cron": return "clock"
        case "hook": return "link"
        case "subagent": return "person.2"
        default: return "bubble.left"
        }
    }
}
