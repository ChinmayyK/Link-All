// DesignSystem.swift — Link All macOS v4
// Canonical design tokens, components, and view modifiers.
// Updated: full adaptive light/dark sidebar + refined component polish.

import SwiftUI
import AppKit

// MARK: - Adaptive Color

extension Color {
    init(light: Color, dark: Color) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
                ? NSColor(dark) : NSColor(light)
        })
    }
    init(hex: UInt32, opacity: Double = 1) {
        self.init(
            red:   Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8)  & 0xFF) / 255,
            blue:  Double(hex         & 0xFF) / 255,
            opacity: opacity)
    }
    
    // Helper to force light mode aesthetic while keeping the API
    init(lightOnly hex: UInt32, opacity: Double = 1) {
        self.init(hex: hex, opacity: opacity)
    }
}

// MARK: - CRTheme

enum CRTheme {

    // ── Brand ─────────────────────────────────────────────────────────────────
    static let brandElectric = Color(hex: 0x0055CC) // Native blue
    static let brandViolet   = Color(hex: 0x5E5CE6) // Native purple
    static let brandCyan     = Color(hex: 0x32ADE6)
    static let brandPink     = Color(hex: 0xFF2D55)

    // ── Semantic surfaces ─────────────────────────────────────────────────────
    static var surface:         Color { Color(light: Color(hex: 0xFFFFFF), dark: Color(hex: 0x000000)) }
    static var surfaceStrong:   Color { Color(light: Color(hex: 0xF5F5F5), dark: Color(hex: 0x0A0A0A)) }
    static var surfaceElevated: Color { Color(light: Color(hex: 0xFFFFFF), dark: Color(hex: 0x141414)) }

    // ── Row states ────────────────────────────────────────────────────────────
    static var rowHover:    Color { Color(light: Color(hex: 0x000000, opacity: 0.04), dark: Color(hex: 0xFFFFFF, opacity: 0.05)) }
    static var rowSelected: Color { Color(light: Color(hex: 0x000000, opacity: 0.08), dark: Color(hex: 0xFFFFFF, opacity: 0.10)) }

    // ── System accent palette ─────────────────────────────────────────────────
    static let accentBlue   = Color(hex: 0x0055CC)
    static let accentGreen  = Color(hex: 0x34C759)
    static let accentYellow = Color(hex: 0xFFCC00)
    static let accentOrange = Color(hex: 0xFF9500)
    static let accentRed    = Color(hex: 0xFF3B30)
    static let accentPurple = Color(hex: 0x5E5CE6)
    static let accentIndigo = Color(hex: 0x5856D6)
    static let accentTeal   = Color(hex: 0x30B0C7)
    static let accentMint   = Color(hex: 0x00C7BE)
    static let accentPink   = Color(hex: 0xFF2D55)
    static let accentGold   = Color(hex: 0xAF52DE)

    // ── Text ──────────────────────────────────────────────────────────────────
    static var ink:       Color { Color(light: Color(hex: 0x000000, opacity: 0.85), dark: Color(hex: 0xFFFFFF, opacity: 0.85)) }
    static var inkSoft:   Color { Color(light: Color(hex: 0x000000, opacity: 0.55), dark: Color(hex: 0xFFFFFF, opacity: 0.55)) }
    static var inkSubtle: Color { Color(light: Color(hex: 0x000000, opacity: 0.40), dark: Color(hex: 0xFFFFFF, opacity: 0.40)) }
    static var inkFaint:  Color { Color(light: Color(hex: 0x000000, opacity: 0.25), dark: Color(hex: 0xFFFFFF, opacity: 0.25)) }

    // ── Borders ───────────────────────────────────────────────────────────────
    static var stroke:     Color { Color(light: Color(hex: 0x000000, opacity: 0.10), dark: Color(hex: 0xFFFFFF, opacity: 0.10)) }
    static var strokeSoft: Color { Color(light: Color(hex: 0x000000, opacity: 0.05), dark: Color(hex: 0xFFFFFF, opacity: 0.05)) }

    // ── Legacy Mappings (Redirect to strict surfaces) ─────────────────────────
    static var sidebarBase: Color { surfaceElevated }
    static var sidebarMid:  Color { surfaceElevated }
    static var sidebarTop:  Color { surfaceElevated }
    static var sidebarInk: Color { ink }
    static var sidebarInkSoft: Color { inkSoft }
    static var sidebarInkSubtle: Color { inkSubtle }
    static var sidebarDivider: Color { stroke }
    static var sidebarSelectedFill: Color { rowSelected }
    static var sidebarHoverFill: Color { rowHover }
    static var sidebarSelectedStroke: Color { stroke }
    static var sidebarSelectedInk: Color { ink }
    static var sidebarSelectedAccent: Color { accentBlue }
    static var sidebarPillFill: Color { rowHover }
    static var sidebarPillStroke: Color { strokeSoft }
    static var canvasTop: Color { surface }
    static var canvasBottom: Color { surface }
    
    static var brandGradient: LinearGradient { LinearGradient(colors: [brandElectric, brandElectric], startPoint: .top, endPoint: .bottom) }
    static var cardGradient: LinearGradient { LinearGradient(colors: [surfaceElevated, surfaceElevated], startPoint: .top, endPoint: .bottom) }
    static var canvasGradient: LinearGradient { LinearGradient(colors: [surface, surface], startPoint: .top, endPoint: .bottom) }
    static var sidebarOverlay: LinearGradient { LinearGradient(colors: [surfaceElevated, surfaceElevated], startPoint: .top, endPoint: .bottom) }

    static var backgroundGradient: LinearGradient { canvasGradient }
    static var backgroundTop:      Color           { surface }
    static var backgroundBottom:   Color           { surface }
    static var sidebarTop_light:   Color           { surfaceElevated }
    static var sidebarBottom:      Color           { surfaceElevated }
    static var sidebarGradient:    LinearGradient  { sidebarOverlay }

    // ── Corner Radii ────────────────────────────────────────────────────────
    static let radiusSmall: CGFloat = 8
    static let radiusMedium: CGFloat = 12
    static let radiusLarge: CGFloat = 16
}

typealias PBTheme = CRTheme

// MARK: - Typography

enum CRFont {
    static func outfit(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .custom("Outfit-\(weightString(weight))", size: size)
    }
    static func inter(size: CGFloat, weight: Font.Weight = .regular) -> Font {
        .custom("Inter-\(weightString(weight))", size: size)
    }
    
    private static func weightString(_ weight: Font.Weight) -> String {
        switch weight {
        case .black: return "Black"
        case .heavy: return "ExtraBold"
        case .bold: return "Bold"
        case .semibold: return "SemiBold"
        case .medium: return "Medium"
        case .regular: return "Regular"
        case .light: return "Light"
        case .thin: return "Thin"
        default: return "Regular"
        }
    }
}

// MARK: - Density Mode

enum CRDensityMode {
    case compact, comfortable
    var rowPadding:  CGFloat { self == .compact ? 8 : 12 }
    var cardSpacing: CGFloat { self == .compact ? 6  : 8 }
    var cardRadius:  CGFloat { 8 }
}

// MARK: - Animation

extension Animation {
    static let crSpring = Animation.interactiveSpring(response: 0.20, dampingFraction: 0.80, blendDuration: 0.1)
    static let crFast   = Animation.interactiveSpring(response: 0.14, dampingFraction: 0.85, blendDuration: 0.1)
    static let crSlow   = Animation.spring(response: 0.35, dampingFraction: 0.85)
    static let crBounce = Animation.interactiveSpring(response: 0.25, dampingFraction: 0.65, blendDuration: 0.1)
}

// MARK: - NSVisualEffectView Wrappers

struct CRSidebarMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = .sidebar; v.blendingMode = .behindWindow; v.state = .active
        return v
    }
    func updateNSView(_ v: NSVisualEffectView, context: Context) {}
}

struct CRHUDMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = .popover; v.blendingMode = .behindWindow; v.state = .active
        return v
    }
    func updateNSView(_ v: NSVisualEffectView, context: Context) {}
}

struct CRVisualEffect: NSViewRepresentable {
    var material: NSVisualEffectView.Material = .sidebar
    var blendingMode: NSVisualEffectView.BlendingMode = .behindWindow
    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = material; v.blendingMode = blendingMode; v.state = .active; return v
    }
    func updateNSView(_ v: NSVisualEffectView, context: Context) {
        v.material = material; v.blendingMode = blendingMode
    }
}

// MARK: - Card Modifier

private struct CRCardModifier: ViewModifier {
    var cornerRadius: CGFloat = 8
    var highlighted:  Bool    = false
    var accentColor:  Color   = CRTheme.accentBlue
    func body(content: Content) -> some View {
        content
            .background {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .fill(CRTheme.surfaceElevated)
                    .overlay {
                        RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                            .strokeBorder(
                                highlighted ? accentColor.opacity(0.80) : CRTheme.stroke.opacity(0.70),
                                lineWidth: highlighted ? 1.5 : 0.5)
                    }
                .shadow(color: highlighted ? accentColor.opacity(0.2) : Color.black.opacity(0.06),
                        radius: highlighted ? 12 : 4, x: 0, y: highlighted ? 4 : 2)
            }
    }
}

extension View {
    func crCard(cornerRadius: CGFloat = 12, highlighted: Bool = false,
                accent: Color = CRTheme.accentBlue) -> some View {
        modifier(CRCardModifier(cornerRadius: cornerRadius, highlighted: highlighted, accentColor: accent))
    }
    func pbCard(cornerRadius: CGFloat = 12, highlighted: Bool = false) -> some View {
        modifier(CRCardModifier(cornerRadius: cornerRadius, highlighted: highlighted))
    }
}

// MARK: - Input Modifier

private struct CRInputModifier: ViewModifier {
    var dark: Bool = false
    var invalid: Bool = false
    func body(content: Content) -> some View {
        content
            .textFieldStyle(.plain)
            .padding(.horizontal, 10).padding(.vertical, 7)
            .background {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(invalid
                          ? CRTheme.accentRed.opacity(0.06)
                          : (dark ? Color(white: 1, opacity: 0.07) : CRTheme.surface))
                    .overlay {
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .strokeBorder(
                                invalid ? CRTheme.accentRed.opacity(0.55) :
                                (dark ? Color(white: 1, opacity: 0.12) : CRTheme.stroke.opacity(0.65)),
                                lineWidth: invalid ? 1.0 : 0.5)
                    }
            }
            .foregroundStyle(dark ? .white : CRTheme.ink)
    }
}

extension View {
    func crInput(dark: Bool = false, invalid: Bool = false) -> some View {
        modifier(CRInputModifier(dark: dark, invalid: invalid))
    }
    func pbInput(dark: Bool = false) -> some View { modifier(CRInputModifier(dark: dark)) }
}

// MARK: - Search Field

struct CRSearchField: View {
    var placeholder: String
    @Binding var text: String
    var shortcut: String? = "⌘F"
    var onClear: (() -> Void)? = nil
    @FocusState private var isFocused: Bool
    @State private var hovered: Bool = false

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 13, weight: isFocused ? .bold : .semibold))
                .foregroundStyle(isFocused ? CRTheme.brandElectric : (hovered ? CRTheme.ink : CRTheme.inkSoft))
                .scaleEffect(isFocused ? 1.08 : 1.0)
                .frame(width: 16)
                .animation(.crSpring, value: isFocused)

            TextField(placeholder, text: $text)
                .textFieldStyle(.plain)
                .font(.system(size: 13, weight: .medium, design: .rounded))
                .foregroundStyle(CRTheme.ink)
                .focused($isFocused)

            if text.isEmpty && !isFocused, let shortcut = shortcut, !shortcut.isEmpty {
                HStack(spacing: 2) {
                    Text(shortcut)
                        .font(.system(size: 10, weight: .bold, design: .rounded))
                }
                .foregroundStyle(CRTheme.inkSubtle)
                .padding(.horizontal, 6)
                .padding(.vertical, 3)
                .background(CRTheme.surfaceElevated.opacity(0.8), in: Capsule())
                .overlay(Capsule().stroke(CRTheme.stroke.opacity(0.4), lineWidth: 0.5))
                .transition(.scale(scale: 0.85).combined(with: .opacity))
            }

            if !text.isEmpty {
                Button {
                    withAnimation(.crFast) { text = "" }
                    onClear?()
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(hovered ? CRTheme.ink : CRTheme.inkSoft)
                }
                .buttonStyle(.plain)
                .transition(.scale(scale: 0.75).combined(with: .opacity))
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background {
            Capsule()
                .fill(CRTheme.surfaceStrong)
                .background(Capsule().fill(.ultraThinMaterial))
                .overlay(
                    Capsule()
                        .strokeBorder(
                            isFocused ? CRTheme.brandElectric : CRTheme.stroke.opacity(0.5),
                            lineWidth: isFocused ? 1.5 : 1.0
                        )
                )
                .shadow(color: isFocused ? CRTheme.brandElectric.opacity(0.2) : .black.opacity(0.05), radius: isFocused ? 5 : 2, y: 1)
        }
        .onHover { hovered = $0 }
        .animation(.crFast, value: isFocused)
        .animation(.crFast, value: text.isEmpty)
    }
}

// MARK: - Glow

struct GlowModifier: ViewModifier {
    var color: Color; var radius: CGFloat
    func body(content: Content) -> some View {
        content
            .shadow(color: color.opacity(0.15), radius: radius / 3, y: radius / 6)
    }
}
extension View {
    func crGlow(_ color: Color, radius: CGFloat = 8) -> some View {
        modifier(GlowModifier(color: color, radius: radius))
    }
}

// MARK: - Button Styles

struct CRPrimaryButtonStyle: ButtonStyle {
    var tint: Color = CRTheme.accentBlue
    @State private var isHovered = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 12).padding(.vertical, 5)
            .background {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(tint).brightness(configuration.isPressed ? -0.05 : (isHovered ? 0.03 : 0))
                    .overlay {
                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.15), lineWidth: 0.5)
                    }
                    .shadow(color: Color.black.opacity(0.08), radius: 2, y: 1)
            }
            .scaleEffect(configuration.isPressed ? 0.98 : 1.0)
            .animation(.crFast, value: configuration.isPressed)
            .animation(.crFast, value: isHovered)
            .onHover { isHovered = $0 }
    }
}

struct CRSecondaryButtonStyle: ButtonStyle {
    @State private var isHovered = false
    @Environment(\.colorScheme) var scheme
    
    func makeBody(configuration: Configuration) -> some View {
        let isDark = scheme == .dark
        configuration.label
            .font(.system(size: 13, weight: .medium))
            .padding(.horizontal, 12).padding(.vertical, 5)
            .background {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(isHovered ? CRTheme.rowHover : (isDark ? Color.white.opacity(0.08) : CRTheme.surface))
                    .overlay {
                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                            .strokeBorder(CRTheme.stroke.opacity(isHovered ? 0.8 : 0.5), lineWidth: 0.5)
                    }
                .shadow(color: Color.black.opacity(isHovered ? 0.05 : 0.02), radius: 2, y: 1)
                .opacity(configuration.isPressed ? 0.7 : 1.0)
            }
            .scaleEffect(configuration.isPressed ? 0.98 : 1.0)
            .animation(.crFast, value: configuration.isPressed)
            .animation(.crFast, value: isHovered)
            .onHover { isHovered = $0 }
    }
}

struct CRDestructiveButtonStyle: ButtonStyle {
    @State private var isHovered = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: .bold))
            .foregroundStyle(isHovered ? .white : CRTheme.accentRed)
            .padding(.horizontal, 14).padding(.vertical, 7)
            .background {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(isHovered ? CRTheme.accentRed : CRTheme.accentRed.opacity(0.08))
                    .overlay {
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .strokeBorder(CRTheme.accentRed.opacity(isHovered ? 0.8 : 0.2), lineWidth: isHovered ? 1.0 : 0.5)
                    }
                    .shadow(color: CRTheme.accentRed.opacity(isHovered ? 0.3 : 0), radius: 6, y: 2)
                    .opacity(configuration.isPressed ? 0.6 : 1.0)
            }
            .scaleEffect(configuration.isPressed ? 0.95 : (isHovered ? 1.02 : 1.0))
            .animation(.crBounce, value: configuration.isPressed)
            .animation(.crSpring, value: isHovered)
            .onHover { isHovered = $0 }
    }
}

typealias PBPrimaryButtonStyle     = CRPrimaryButtonStyle
typealias PBSecondaryButtonStyle   = CRSecondaryButtonStyle
typealias PBDestructiveButtonStyle = CRDestructiveButtonStyle

// MARK: - Tag / Badge

struct CRTag: View {
    let text: String
    let tint: Color
    var filled: Bool = false
    var body: some View {
        Text(text)
            .font(.system(size: 10, weight: .semibold))
            .tracking(0.10)
            .foregroundStyle(filled ? .white : tint.opacity(0.88))
            .padding(.horizontal, 6.5).padding(.vertical, 2.5)
            .background { Capsule().fill(filled ? tint : tint.opacity(0.10)) }
    }
}

/// Adaptive sidebar badge — uses sidebar semantic tokens so it reads in both modes
struct CRNumericBadge: View {
    let count: Int
    var body: some View {
        if count > 0 {
            Text("\(min(count, 99))")
                .font(.system(size: 10, weight: .bold, design: .rounded))
                .foregroundStyle(CRTheme.sidebarInkSoft)
                .padding(.horizontal, 5.5).padding(.vertical, 2)
                .background {
                    Capsule().fill(CRTheme.sidebarPillFill)
                        .overlay { Capsule().strokeBorder(CRTheme.sidebarPillStroke, lineWidth: 0.5) }
                }
        }
    }
}

typealias CRBadge = CRTag
typealias PBBadge = CRTag

// MARK: - Shortcut Hint (sidebar ⌘N labels)

struct CRShortcutHint: View {
    let shortcut: String
    /// Pass `false` in the adaptive sidebar so it uses sidebar semantic colours.
    var dark: Bool = true
    var body: some View {
        Text(shortcut)
            .font(.system(size: 10, weight: .medium, design: .rounded))
            .foregroundStyle(dark ? Color(white: 1, opacity: 0.24) : CRTheme.sidebarInkSubtle)
            .padding(.horizontal, 5).padding(.vertical, 2)
            .background {
                RoundedRectangle(cornerRadius: 4, style: .continuous)
                    .fill(dark ? Color(white: 1, opacity: 0.06) : CRTheme.sidebarPillFill)
                    .overlay {
                        RoundedRectangle(cornerRadius: 4, style: .continuous)
                            .strokeBorder(
                                dark ? Color(white: 1, opacity: 0.08) : CRTheme.sidebarPillStroke,
                                lineWidth: 0.5)
                    }
            }
    }
}

// MARK: - Status Dot

struct StatusDot: View {
    var isOnline: Bool
    var size: CGFloat = 8
    @State private var pulse = false
    var body: some View {
        ZStack {
            if isOnline {
                Circle()
                    .fill(CRTheme.accentGreen.opacity(0.20))
                    .frame(width: size + 9, height: size + 9)
                    .scaleEffect(pulse ? 2.1 : 1.0)
                    .opacity(pulse ? 0 : 0.45)
                    .animation(.easeOut(duration: 1.6).repeatForever(autoreverses: false), value: pulse)
            }
            Circle().fill(isOnline ? CRTheme.accentGreen : CRTheme.inkSubtle).frame(width: size, height: size)
        }
        .onAppear { if isOnline { pulse = true } }
        .onChange(of: isOnline) { v in pulse = v }
    }
}

// MARK: - Device Avatar

struct DeviceAvatar: View {
    let name: String; let platform: String?
    var size: CGFloat = 36; var color: Color = CRTheme.accentBlue
    private var initials: String {
        let w = name.split(separator: " ")
        return w.count >= 2 ? String(w[0].prefix(1) + w[1].prefix(1)).uppercased()
                            : String(name.prefix(2)).uppercased()
    }
    var body: some View {
        ZStack {
            Circle().fill(color.opacity(0.12))
                .overlay { Circle().strokeBorder(color.opacity(0.16), lineWidth: 0.5) }
                .frame(width: size, height: size)
            Text(initials)
                .font(.system(size: size * 0.33, weight: .semibold, design: .rounded))
                .foregroundStyle(color)
        }
    }
}

// MARK: - App Icon Mark

struct CRAppIconMark: View {
    var size: CGFloat = 34
    // The logo for the current appearance: the dark logo in dark mode.
    @Environment(\.colorScheme) private var scheme
    private var logoName: String { scheme == .dark ? "AppIconSourceDark" : "AppIconSource" }
    var body: some View {
        ZStack {
            if let image = NSImage(named: NSImage.Name(logoName)) {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: size, height: size)
            } else if let imagePath = Bundle.main.path(forResource: logoName, ofType: "png"),
                      let image = NSImage(contentsOfFile: imagePath) {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: size, height: size)
            } else {
                // Fallback
                RoundedRectangle(cornerRadius: size * 0.30, style: .continuous)
                    .fill(CRTheme.brandGradient)
                    .frame(width: size, height: size)
                    .shadow(color: CRTheme.brandElectric.opacity(0.42), radius: size * 0.35, y: size * 0.12)
                Image(systemName: "arrow.left.arrow.right.circle.fill")
                    .font(.system(size: size * 0.44, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.96))
                    .symbolRenderingMode(.hierarchical)
            }
        }
    }
}

// MARK: - Icon Chip

struct CRIconChip: View {
    let systemName: String; let tint: Color
    var size: CGFloat = 28; var radius: CGFloat? = nil
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: radius ?? size * 0.32, style: .continuous)
                .fill(tint.opacity(0.10)).frame(width: size, height: size)
            Image(systemName: systemName)
                .font(.system(size: size * 0.42, weight: .semibold))
                .foregroundStyle(tint).symbolRenderingMode(.hierarchical)
        }
    }
}

// MARK: - Section Header

struct CRSectionHeader: View {
    let eyebrow: String; let title: String; var subtitle: String? = nil
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 5) {
                Capsule().fill(CRTheme.brandGradient).frame(width: 2.5, height: 11)
                Text(eyebrow.uppercased())
                    .font(.system(size: 10, weight: .bold)).tracking(1.0)
                    .foregroundStyle(CRTheme.brandElectric)
            }
            Text(title)
                .font(.system(size: 24, weight: .bold)).foregroundStyle(CRTheme.ink).tracking(-0.4)
            if let subtitle {
                Text(subtitle).font(.system(size: 13)).foregroundStyle(CRTheme.inkSoft)
                    .fixedSize(horizontal: false, vertical: true).lineSpacing(1.5)
            }
        }
    }
}

// MARK: - Empty State

struct CREmptyState: View {
    let systemImage: String; let title: String; let message: String
    var accent: Color = CRTheme.accentBlue
    var actionLabel: String? = nil
    var onAction: (() -> Void)? = nil
    @State private var isFloating = false

    var body: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle().fill(accent.opacity(0.06)).frame(width: 72, height: 72)
                Circle().strokeBorder(accent.opacity(0.10), lineWidth: 0.5).frame(width: 72, height: 72)
                Image(systemName: systemImage)
                    .font(.system(size: 24, weight: .light)).foregroundStyle(accent.opacity(0.48))
                    .symbolRenderingMode(.hierarchical)
                    .offset(y: isFloating ? -4 : 4)
                    .animation(.easeInOut(duration: 2.0).repeatForever(autoreverses: true), value: isFloating)
            }
            .onAppear { isFloating = true }
            VStack(spacing: 5) {
                Text(title).font(.system(size: 13.5, weight: .semibold)).foregroundStyle(CRTheme.ink)
                Text(message).font(.system(size: 12)).foregroundStyle(CRTheme.inkSoft)
                    .multilineTextAlignment(.center).frame(maxWidth: 280).lineSpacing(2)
            }
            if let label = actionLabel, let action = onAction {
                Button(label, action: action).buttonStyle(CRPrimaryButtonStyle(tint: accent))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).padding(.vertical, 48)
    }
}

// MARK: - Toast Stack

struct CRToastStack: View {
    let toasts: [ToastItem]
    var body: some View {
        VStack(alignment: .trailing, spacing: 6) {
            ForEach(toasts.suffix(3)) { toast in
                HStack(spacing: 9) {
                    RoundedRectangle(cornerRadius: 2).fill(toast.tint).frame(width: 3, height: 30)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(toast.title).font(.system(size: 12.5, weight: .semibold)).foregroundStyle(CRTheme.ink)
                        Text(toast.body).font(.system(size: 11)).foregroundStyle(CRTheme.inkSoft)
                    }
                    Spacer(minLength: 0)
                }
                .padding(.leading, 4).padding(.trailing, 14).padding(.vertical, 8)
                .background {
                    CRHUDMaterial()
                        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
                        .overlay {
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .strokeBorder(Color.white.opacity(0.15), lineWidth: 0.5)
                        }
                        .shadow(color: .black.opacity(0.15), radius: 16, y: 8)
                }
                .frame(maxWidth: 270)
                .transition(.asymmetric(insertion: .move(edge: .trailing).combined(with: .opacity),
                                        removal:   .move(edge: .trailing).combined(with: .opacity)))
            }
        }
        .animation(.crSpring, value: toasts.count)
    }
}

// MARK: - Sidebar Nav Button

struct SidebarNavButton: View {
    let icon: String; let label: String
    var badge: Int = 0; var shortcut: String = ""
    var isSelected: Bool; let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: {
            if !isSelected { NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .default) }
            action()
        }) {
            HStack(spacing: 8) {
                Image(systemName: isSelected ? (icon + ".fill") : icon)
                    .font(.system(size: 13.5, weight: isSelected ? .semibold : .regular))
                    .foregroundStyle(isSelected ? CRTheme.sidebarSelectedAccent : CRTheme.sidebarInkSoft)
                    .symbolRenderingMode(.hierarchical)
                    .frame(width: 18, alignment: .center)

                Text(label)
                    .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
                    .foregroundStyle(isSelected ? CRTheme.sidebarSelectedInk : CRTheme.sidebarInkSoft)

                Spacer(minLength: 0)

                if hovered && !shortcut.isEmpty {
                    CRShortcutHint(shortcut: shortcut, dark: false)
                        .transition(.opacity.combined(with: .scale(scale: 0.9)))
                } else {
                    CRNumericBadge(count: badge)
                }
            }
            .padding(.horizontal, 10).padding(.vertical, 7)
            .frame(maxWidth: .infinity, minHeight: 30)
            .background {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(isSelected ? CRTheme.sidebarSelectedFill
                                     : (hovered ? CRTheme.sidebarHoverFill : .clear))
                    .overlay {
                        if isSelected {
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .strokeBorder(CRTheme.sidebarSelectedStroke, lineWidth: 1.0)
                        }
                    }
                    .shadow(color: isSelected ? CRTheme.brandElectric.opacity(0.15) : .clear,
                            radius: isSelected ? 4 : 0, x: 0, y: isSelected ? 2 : 0)
            }
        }
        .buttonStyle(.plain)
        .scaleEffect(hovered && !isSelected ? 1.02 : 1.0)
        .onHover { hovered = $0 }
        .animation(.crBounce, value: isSelected)
        .animation(.crSpring, value: hovered)
    }
}

// MARK: - Sidebar Stat Pill

struct SidebarStatPill: View {
    let icon: String; let value: String; let label: String
    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: icon).font(.system(size: 9, weight: .semibold))
                .foregroundStyle(CRTheme.sidebarInkSubtle)
            Text(value).font(.system(size: 12, weight: .bold, design: .rounded))
                .foregroundStyle(CRTheme.sidebarInk.opacity(0.80))
            Text(label).font(.system(size: 10)).foregroundStyle(CRTheme.sidebarInkSubtle)
        }
        .padding(.horizontal, 9).padding(.vertical, 5)
        .background {
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(CRTheme.sidebarPillFill)
                .overlay {
                    RoundedRectangle(cornerRadius: 7, style: .continuous)
                        .strokeBorder(CRTheme.sidebarPillStroke, lineWidth: 0.5)
                }
        }
    }
}

struct DevicePill: View {
    let text: String; let tint: Color
    var body: some View { CRTag(text: text, tint: tint) }
}

// MARK: - Dividers

struct CRDivider: View {
    var inset: CGFloat = 0
    var body: some View {
        Rectangle().fill(CRTheme.stroke.opacity(0.45)).frame(height: 0.5).padding(.leading, inset)
    }
}

/// Adaptive divider for use inside the sidebar
struct CRDividerDark: View {
    var inset: CGFloat = 0
    var body: some View {
        Rectangle().fill(CRTheme.sidebarDivider).frame(height: 0.5).padding(.leading, inset)
    }
}

// MARK: - Keyboard Chip

struct KbdChip: View {
    let key: String; var dark: Bool = true
    init(_ key: String, dark: Bool = true) { self.key = key; self.dark = dark }
    var body: some View {
        Text(key)
            .font(.system(size: 10.5, weight: .semibold, design: .rounded))
            .foregroundStyle(dark ? Color(white: 1, opacity: 0.28) : CRTheme.inkSubtle)
            .padding(.horizontal, 5.5).padding(.vertical, 2.5)
            .background {
                RoundedRectangle(cornerRadius: 5, style: .continuous)
                    .fill(dark ? Color(white: 1, opacity: 0.07) : CRTheme.surface)
                    .overlay {
                        RoundedRectangle(cornerRadius: 5, style: .continuous)
                            .strokeBorder(dark ? Color(white: 1, opacity: 0.10) : CRTheme.stroke.opacity(0.55),
                                          lineWidth: 0.5)
                    }
            }
    }
}

// MARK: - Inline Progress Bar

struct CRProgressBar: View {
    var value: Double; var tint: Color = CRTheme.accentIndigo; var height: CGFloat = 3
    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(tint.opacity(0.14)).frame(height: height)
                Capsule().fill(tint)
                    .frame(width: geo.size.width * max(0, min(value, 1)), height: height)
                    .animation(.crSpring, value: value)
            }
        }
        .frame(height: height)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Progress")
        .accessibilityValue("\(Int(max(0, min(value, 1)) * 100))%")
    }
}

// MARK: - Day Header (for grouped lists)

struct CRDayHeader: View {
    let date: Date
    private var label: String {
        if Calendar.current.isDateInToday(date)     { return "Today" }
        if Calendar.current.isDateInYesterday(date) { return "Yesterday" }
        let f = DateFormatter()
        f.dateFormat = Calendar.current.component(.year, from: date) == Calendar.current.component(.year, from: Date())
            ? "EEEE, MMMM d" : "MMMM d, yyyy"
        return f.string(from: date)
    }
    var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(CRTheme.inkSubtle)
            Rectangle().fill(CRTheme.stroke.opacity(0.35)).frame(height: 0.5)
        }
        .padding(.vertical, 6)
    }
}

// MARK: - Section Toolbar (sticky top bar for detail panes)

struct CRSectionToolbar<TrailingContent: View>: View {
    let title:    String
    let subtitle: String
    @ViewBuilder var trailing: () -> TrailingContent

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(title).font(.system(size: 15, weight: .semibold)).foregroundStyle(CRTheme.ink)
                    Text(subtitle).font(.system(size: 11)).foregroundStyle(CRTheme.inkSoft)
                }
                Spacer()
                trailing()
            }
            .padding(.horizontal, 20).padding(.vertical, 11)
            CRDivider()
        }
        .background(CRTheme.surfaceElevated)
    }
}

// MARK: - Fluid Background

struct CRFluidBackgroundView: View {
    @State private var isAnimating = false

    var body: some View {
        ZStack {
            CRTheme.surface.ignoresSafeArea()
            
            GeometryReader { geo in
                ZStack {
                    Circle()
                        .fill(CRTheme.brandElectric.opacity(0.08))
                        .frame(width: geo.size.width * 0.8)
                        .offset(x: geo.size.width * 0.2)
                        .rotationEffect(.degrees(isAnimating ? 360 : 0))
                        .blur(radius: 80)
                        .animation(.linear(duration: 20.0).repeatForever(autoreverses: false), value: isAnimating)
                    
                    Circle()
                        .fill(CRTheme.brandViolet.opacity(0.06))
                        .frame(width: geo.size.width * 0.6)
                        .offset(x: geo.size.width * 0.3)
                        .rotationEffect(.degrees(isAnimating ? -360 : 0))
                        .blur(radius: 60)
                        .animation(.linear(duration: 25.0).repeatForever(autoreverses: false), value: isAnimating)
                }
                .frame(width: geo.size.width, height: geo.size.height)
            }
        }
        .ignoresSafeArea()
        .onAppear {
            isAnimating = true
        }
    }
}

// MARK: - Spotlight Hover Effect

struct SpotlightHoverModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
    }
}

extension View {
    func spotlightHover() -> some View {
        self.modifier(SpotlightHoverModifier())
    }
}

// MARK: - Hover Scale

struct CRHoverScaleModifier: ViewModifier {
    let scale: CGFloat
    @State private var isHovered = false
    
    func body(content: Content) -> some View {
        content
            .scaleEffect(isHovered ? scale : 1.0)
            .animation(.crSpring, value: isHovered)
            .onHover { isHovered = $0 }
    }
}

extension View {
    func crHoverScale(scale: CGFloat = 1.02) -> some View {
        modifier(CRHoverScaleModifier(scale: scale))
    }
}

// MARK: - Hover Scale ButtonStyle

public struct HoverScaleStyle: ButtonStyle {
    public var scale: CGFloat = 1.02
    @State private var isHovered = false
    
    public func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.96 : (isHovered ? scale : 1.0))
            .animation(.crSpring, value: isHovered)
            .animation(.crFast, value: configuration.isPressed)
            .onHover { isHovered = $0 }
    }
}

// MARK: - Typography (Dynamic Type)

struct CRFontModifier: ViewModifier {
    @ScaledMetric var size: CGFloat
    var weight: Font.Weight
    var design: Font.Design

    init(size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default) {
        self._size = ScaledMetric(wrappedValue: size)
        self.weight = weight
        self.design = design
    }

    func body(content: Content) -> some View {
        content.font(.system(size: size, weight: weight, design: design))
    }
}

extension View {
    func crFont(size: CGFloat, weight: Font.Weight = .regular, design: Font.Design = .default) -> some View {
        modifier(CRFontModifier(size: size, weight: weight, design: design))
    }
}
