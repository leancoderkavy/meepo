import SwiftUI

struct ContentView: View {
    @StateObject private var settings = SettingsStore()
    @State private var client: MeepoClient?
    @State private var viewModel: ChatViewModel?
    @State private var selectedTab: Tab = .chat

    enum Tab: Hashable {
        case chat
        case sessions
        case settings
    }

    var body: some View {
        Group {
            if let client, let viewModel {
                TabView(selection: $selectedTab) {
                    NavigationStack {
                        ChatView(viewModel: viewModel, client: client)
                    }
                    .tabItem {
                        Label("Chat", systemImage: "bubble.left.and.bubble.right")
                    }
                    .tag(Tab.chat)

                    NavigationStack {
                        SessionsView(viewModel: viewModel, client: client)
                    }
                    .tabItem {
                        Label("Tunnels", systemImage: "point.3.connected.trianglepath.dotted")
                    }
                    .tag(Tab.sessions)

                    NavigationStack {
                        SettingsView(settings: settings, client: client)
                    }
                    .tabItem {
                        Label("Settings", systemImage: "gearshape")
                    }
                    .tag(Tab.settings)
                }
                .tint(MeepoTheme.warmTan)
                .onAppear {
                    // Theme the tab bar
                    let tabBarAppearance = UITabBarAppearance()
                    tabBarAppearance.configureWithOpaqueBackground()
                    tabBarAppearance.backgroundColor = UIColor(MeepoTheme.barBackground)

                    // Normal state
                    tabBarAppearance.stackedLayoutAppearance.normal.iconColor = UIColor(MeepoTheme.shadow)
                    tabBarAppearance.stackedLayoutAppearance.normal.titleTextAttributes = [
                        .foregroundColor: UIColor(MeepoTheme.shadow)
                    ]

                    // Selected state
                    tabBarAppearance.stackedLayoutAppearance.selected.iconColor = UIColor(MeepoTheme.warmTan)
                    tabBarAppearance.stackedLayoutAppearance.selected.titleTextAttributes = [
                        .foregroundColor: UIColor(MeepoTheme.warmTan)
                    ]

                    UITabBar.appearance().standardAppearance = tabBarAppearance
                    UITabBar.appearance().scrollEdgeAppearance = tabBarAppearance
                }
            } else {
                // Splash / loading state
                ZStack {
                    MeepoTheme.caveDark.ignoresSafeArea()
                    VStack(spacing: 20) {
                        MeepoAvatar(size: 72)
                        Text("Meepo")
                            .font(.title.bold())
                            .foregroundStyle(MeepoTheme.parchment)
                        ProgressView()
                            .tint(MeepoTheme.warmTan)
                    }
                }
            }
        }
        .onAppear {
            // Theme navigation bars globally
            let navAppearance = UINavigationBarAppearance()
            navAppearance.configureWithOpaqueBackground()
            navAppearance.backgroundColor = UIColor(MeepoTheme.barBackground)
            navAppearance.titleTextAttributes = [
                .foregroundColor: UIColor(MeepoTheme.parchment)
            ]
            navAppearance.largeTitleTextAttributes = [
                .foregroundColor: UIColor(MeepoTheme.parchment)
            ]
            UINavigationBar.appearance().standardAppearance = navAppearance
            UINavigationBar.appearance().scrollEdgeAppearance = navAppearance
            UINavigationBar.appearance().compactAppearance = navAppearance

            let c = MeepoClient(settings: settings)
            client = c
            viewModel = ChatViewModel(client: c, settings: settings)

            if settings.isConfigured {
                c.connect()
            }
        }
    }
}

#Preview {
    ContentView()
}
