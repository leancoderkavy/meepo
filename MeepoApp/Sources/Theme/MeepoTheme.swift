import SwiftUI

// MARK: - Meepo Color Palette
// Inspired by the Dota 2 Meepo character:
// Blue hood, earthy brown skin, warm tan fur, dark underground caves

enum MeepoTheme {

    // ── Primary Colors ──

    /// Meepo's iconic blue hood
    static let hoodBlue = Color(light: .init(r: 0.231, g: 0.420, b: 0.541),
                                dark: .init(r: 0.290, g: 0.541, b: 0.710))

    /// Deep earth brown — leather, dirt, underground
    static let earthBrown = Color(light: .init(r: 0.545, g: 0.412, b: 0.078),
                                  dark: .init(r: 0.420, g: 0.306, b: 0.106))

    /// Warm tan — fur tufts, highlights
    static let warmTan = Color(light: .init(r: 0.769, g: 0.635, b: 0.396),
                               dark: .init(r: 0.831, g: 0.722, b: 0.478))

    /// Gold accent — teeth, treasure, shovel gleam
    static let goldAccent = Color(light: .init(r: 0.831, g: 0.659, b: 0.263),
                                  dark: .init(r: 0.878, g: 0.718, b: 0.333))

    // ── Background Colors ──

    /// Deep cave background
    static let caveDark = Color(light: .init(r: 0.949, g: 0.929, b: 0.898),
                                dark: .init(r: 0.102, g: 0.078, b: 0.063))

    /// Slightly lighter cave surface (cards, bubbles)
    static let caveStone = Color(light: .init(r: 0.918, g: 0.894, b: 0.859),
                                 dark: .init(r: 0.165, g: 0.141, b: 0.125))

    /// Cave wall — elevated surfaces
    static let caveWall = Color(light: .init(r: 0.878, g: 0.851, b: 0.812),
                                dark: .init(r: 0.227, g: 0.188, b: 0.157))

    // ── Text Colors ──

    /// Parchment text on dark backgrounds
    static let parchment = Color(light: .init(r: 0.180, g: 0.145, b: 0.110),
                                 dark: .init(r: 0.910, g: 0.863, b: 0.784))

    /// Secondary text
    static let dusty = Color(light: .init(r: 0.420, g: 0.380, b: 0.330),
                             dark: .init(r: 0.620, g: 0.576, b: 0.510))

    /// Tertiary / muted text
    static let shadow = Color(light: .init(r: 0.580, g: 0.545, b: 0.498),
                              dark: .init(r: 0.440, g: 0.400, b: 0.353))

    // ── Semantic Colors ──

    /// User message bubble
    static let userBubble = hoodBlue

    /// Assistant message bubble
    static let assistantBubble = caveWall

    /// Input field background
    static let inputField = caveStone

    /// Tab bar / navigation bar
    static let barBackground = Color(light: .init(r: 0.933, g: 0.910, b: 0.875),
                                     dark: .init(r: 0.118, g: 0.094, b: 0.075))

    /// Divider / separator
    static let divider = Color(light: .init(r: 0.820, g: 0.790, b: 0.745),
                               dark: .init(r: 0.250, g: 0.216, b: 0.184))

    /// Success / connected
    static let gemGreen = Color(light: .init(r: 0.298, g: 0.588, b: 0.314),
                                dark: .init(r: 0.376, g: 0.686, b: 0.392))

    /// Error / disconnected
    static let bloodRed = Color(light: .init(r: 0.698, g: 0.220, b: 0.180),
                                dark: .init(r: 0.820, g: 0.302, b: 0.255))

    /// Warning / connecting
    static let torchOrange = Color(light: .init(r: 0.820, g: 0.545, b: 0.180),
                                   dark: .init(r: 0.878, g: 0.620, b: 0.255))

    // ── Gradients ──

    static let caveGradient = LinearGradient(
        colors: [caveDark, caveStone],
        startPoint: .top,
        endPoint: .bottom
    )

    static let hoodGradient = LinearGradient(
        colors: [hoodBlue, hoodBlue.opacity(0.8)],
        startPoint: .topLeading,
        endPoint: .bottomTrailing
    )

    // ── Corner Radius ──

    static let bubbleRadius: CGFloat = 18
    static let cardRadius: CGFloat = 14
    static let inputRadius: CGFloat = 22
    static let badgeRadius: CGFloat = 8
}

// MARK: - Adaptive Color Helper

extension Color {
    init(light: Color.Resolved, dark: Color.Resolved) {
        self.init(UIColor { traits in
            if traits.userInterfaceStyle == .dark {
                return UIColor(
                    red: CGFloat(dark.red),
                    green: CGFloat(dark.green),
                    blue: CGFloat(dark.blue),
                    alpha: CGFloat(dark.opacity)
                )
            } else {
                return UIColor(
                    red: CGFloat(light.red),
                    green: CGFloat(light.green),
                    blue: CGFloat(light.blue),
                    alpha: CGFloat(light.opacity)
                )
            }
        })
    }
}

extension Color.Resolved {
    init(r: Float, g: Float, b: Float, a: Float = 1.0) {
        self.init(red: r, green: g, blue: b, opacity: a)
    }
}

// MARK: - View Modifiers

struct MeepoBackground: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background(MeepoTheme.caveDark)
    }
}

struct MeepoCardStyle: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background(MeepoTheme.caveStone)
            .clipShape(RoundedRectangle(cornerRadius: MeepoTheme.cardRadius, style: .continuous))
    }
}

extension View {
    func meepoBackground() -> some View {
        modifier(MeepoBackground())
    }

    func meepoCard() -> some View {
        modifier(MeepoCardStyle())
    }
}
