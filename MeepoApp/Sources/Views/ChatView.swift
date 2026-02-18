import SwiftUI

struct ChatView: View {
    @ObservedObject var viewModel: ChatViewModel
    @ObservedObject var client: MeepoClient
    @FocusState private var isInputFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            // Connection banner
            if !client.connectionState.isConnected {
                ConnectionBanner(state: client.connectionState) {
                    viewModel.connect()
                }
            }

            // Messages
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 14) {
                        // Empty state
                        if viewModel.messages.isEmpty && client.connectionState.isConnected {
                            MeepoEmptyChat()
                                .padding(.top, 60)
                        }

                        ForEach(viewModel.messages) { message in
                            MessageBubble(message: message)
                                .id(message.id)
                        }

                        // Typing indicator
                        if client.isTyping {
                            TypingIndicator(toolName: client.currentTool)
                                .id("typing")
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                }
                .background(MeepoTheme.caveDark)
                .onChange(of: viewModel.messages.count) {
                    withAnimation(.easeOut(duration: 0.2)) {
                        if client.isTyping {
                            proxy.scrollTo("typing", anchor: .bottom)
                        } else if let last = viewModel.messages.last {
                            proxy.scrollTo(last.id, anchor: .bottom)
                        }
                    }
                }
                .onChange(of: client.isTyping) {
                    if client.isTyping {
                        withAnimation {
                            proxy.scrollTo("typing", anchor: .bottom)
                        }
                    }
                }
            }

            // Divider
            Rectangle()
                .fill(MeepoTheme.divider)
                .frame(height: 0.5)

            // Input bar
            InputBar(
                text: $viewModel.inputText,
                isSending: viewModel.isSending,
                isConnected: client.connectionState.isConnected,
                isFocused: $isInputFocused
            ) {
                viewModel.send()
            }
        }
        .background(MeepoTheme.caveDark)
        .navigationTitle(sessionTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(MeepoTheme.barBackground, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                HStack(spacing: 6) {
                    Circle()
                        .fill(connectionDotColor)
                        .frame(width: 8, height: 8)
                    Text("Meepo")
                        .font(.headline)
                        .foregroundStyle(MeepoTheme.parchment)
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Clear Chat", systemImage: "trash") {
                        viewModel.clearMessages()
                    }
                    if client.connectionState.isConnected {
                        Button("Disconnect", systemImage: "wifi.slash") {
                            viewModel.disconnect()
                        }
                    } else {
                        Button("Connect", systemImage: "wifi") {
                            viewModel.connect()
                        }
                    }
                } label: {
                    Image(systemName: "ellipsis.circle")
                        .foregroundStyle(MeepoTheme.warmTan)
                }
            }
        }
    }

    private var sessionTitle: String {
        if let session = viewModel.sessions.first(where: { $0.id == viewModel.currentSessionId }) {
            return session.displayName
        }
        return ""
    }

    private var connectionDotColor: Color {
        switch client.connectionState {
        case .connected: return MeepoTheme.gemGreen
        case .connecting: return MeepoTheme.torchOrange
        case .error: return MeepoTheme.bloodRed
        case .disconnected: return MeepoTheme.shadow
        }
    }
}

// MARK: - Empty Chat State

struct MeepoEmptyChat: View {
    var body: some View {
        VStack(spacing: 16) {
            // Meepo shovel icon
            ZStack {
                Circle()
                    .fill(MeepoTheme.caveWall)
                    .frame(width: 80, height: 80)
                Text("⛏")
                    .font(.system(size: 36))
            }

            Text("Dig In!")
                .font(.title3.bold())
                .foregroundStyle(MeepoTheme.warmTan)

            Text("Send a message to start chatting\nwith your Meepo agent")
                .font(.subheadline)
                .foregroundStyle(MeepoTheme.dusty)
                .multilineTextAlignment(.center)
        }
        .padding()
    }
}

// MARK: - Connection Banner

struct ConnectionBanner: View {
    let state: ConnectionState
    let onConnect: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .foregroundStyle(color)
                .font(.caption)

            Text(state.statusText)
                .font(.caption)
                .foregroundStyle(MeepoTheme.dusty)

            Spacer()

            if case .disconnected = state {
                Button("Connect") { onConnect() }
                    .font(.caption.bold())
                    .foregroundStyle(.white)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 4)
                    .background(MeepoTheme.hoodBlue)
                    .clipShape(Capsule())
            }

            if case .error = state {
                Button("Retry") { onConnect() }
                    .font(.caption.bold())
                    .foregroundStyle(.white)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 4)
                    .background(MeepoTheme.hoodBlue)
                    .clipShape(Capsule())
            }

            if case .connecting = state {
                ProgressView()
                    .controlSize(.mini)
                    .tint(MeepoTheme.warmTan)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(MeepoTheme.caveStone)
    }

    private var icon: String {
        switch state {
        case .disconnected: return "wifi.slash"
        case .connecting: return "wifi"
        case .connected: return "wifi"
        case .error: return "exclamationmark.triangle"
        }
    }

    private var color: Color {
        switch state {
        case .disconnected: return MeepoTheme.shadow
        case .connecting: return MeepoTheme.torchOrange
        case .connected: return MeepoTheme.gemGreen
        case .error: return MeepoTheme.bloodRed
        }
    }
}

// MARK: - Message Bubble

struct MessageBubble: View {
    let message: ChatMessage

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            if message.role.isUser { Spacer(minLength: 48) }

            if !message.role.isUser && message.role != .system {
                // Meepo avatar for assistant messages
                MeepoAvatar(size: 28)
            }

            VStack(alignment: message.role.isUser ? .trailing : .leading, spacing: 4) {
                if message.role == .system {
                    systemBubble
                } else if message.role == .tool {
                    toolBubble
                } else {
                    chatBubble
                }

                Text(message.timestamp, style: .time)
                    .font(.caption2)
                    .foregroundStyle(MeepoTheme.shadow)
            }

            if !message.role.isUser { Spacer(minLength: 48) }
        }
    }

    private var chatBubble: some View {
        Text(message.content)
            .font(.body)
            .foregroundStyle(message.role.isUser ? Color.white : MeepoTheme.parchment)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(
                message.role.isUser
                    ? MeepoTheme.hoodBlue
                    : MeepoTheme.caveWall
            )
            .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.bubbleRadius, style: .continuous))
            .textSelection(.enabled)
    }

    private var toolBubble: some View {
        HStack(spacing: 6) {
            Image(systemName: "wrench.and.screwdriver")
                .font(.caption)
                .foregroundStyle(MeepoTheme.goldAccent)
            Text(message.content)
                .font(.caption)
                .foregroundStyle(MeepoTheme.dusty)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(MeepoTheme.caveStone)
        .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.badgeRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: MeepoTheme.badgeRadius, style: .continuous)
                .strokeBorder(MeepoTheme.divider, lineWidth: 0.5)
        )
    }

    private var systemBubble: some View {
        Text(message.content)
            .font(.caption)
            .foregroundStyle(MeepoTheme.dusty)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(MeepoTheme.caveStone.opacity(0.6))
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
    }
}

// MARK: - Meepo Avatar

struct MeepoAvatar: View {
    var size: CGFloat = 32

    var body: some View {
        ZStack {
            Circle()
                .fill(MeepoTheme.earthBrown)
            Text("⛏")
                .font(.system(size: size * 0.45))
        }
        .frame(width: size, height: size)
    }
}

// MARK: - Typing Indicator

struct TypingIndicator: View {
    let toolName: String?
    @State private var phase: Int = 0

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            MeepoAvatar(size: 28)

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 5) {
                    ForEach(0..<3, id: \.self) { i in
                        Circle()
                            .fill(MeepoTheme.warmTan)
                            .frame(width: 7, height: 7)
                            .scaleEffect(phase == i ? 1.3 : 0.7)
                            .animation(
                                .easeInOut(duration: 0.5)
                                    .repeatForever()
                                    .delay(Double(i) * 0.15),
                                value: phase
                            )
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(MeepoTheme.caveWall)
                .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.bubbleRadius, style: .continuous))

                if let toolName {
                    Label("Digging with \(toolName)…", systemImage: "gearshape.2")
                        .font(.caption2)
                        .foregroundStyle(MeepoTheme.goldAccent)
                }
            }

            Spacer(minLength: 48)
        }
        .onAppear { phase = 2 }
    }
}

// MARK: - Input Bar

struct InputBar: View {
    @Binding var text: String
    let isSending: Bool
    let isConnected: Bool
    var isFocused: FocusState<Bool>.Binding

    let onSend: () -> Void

    private var canSend: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !isSending && isConnected
    }

    var body: some View {
        HStack(alignment: .bottom, spacing: 10) {
            TextField("Message Meepo…", text: $text, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...6)
                .focused(isFocused)
                .disabled(!isConnected)
                .foregroundStyle(MeepoTheme.parchment)
                .onSubmit { onSend() }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(MeepoTheme.caveStone)
                .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.inputRadius, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: MeepoTheme.inputRadius, style: .continuous)
                        .strokeBorder(
                            isInputActive ? MeepoTheme.hoodBlue.opacity(0.5) : MeepoTheme.divider,
                            lineWidth: 1
                        )
                )

            Button(action: onSend) {
                Group {
                    if isSending {
                        ProgressView()
                            .controlSize(.small)
                            .tint(MeepoTheme.warmTan)
                    } else {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.title2)
                            .symbolRenderingMode(.palette)
                            .foregroundStyle(
                                canSend ? Color.white : MeepoTheme.shadow,
                                canSend ? MeepoTheme.hoodBlue : MeepoTheme.caveWall
                            )
                    }
                }
                .frame(width: 36, height: 36)
            }
            .disabled(!canSend)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(MeepoTheme.barBackground)
    }

    private var isInputActive: Bool {
        !text.isEmpty
    }
}
