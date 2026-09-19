import SwiftUI

/// sRGB colors converted from the existing CSS OKLCH tokens; native controls retain system behavior.
struct WhisperPalette {
    let scheme: ColorScheme
    var window: Color { scheme == .dark ? Self.oklch(0.19, 0.005, 260) : Color(hex: 0xf7f7f7) }
    var canvas: Color { scheme == .dark ? Self.oklch(0.22, 0.006, 260) : .white }
    var card: Color { scheme == .dark ? Self.oklch(0.27, 0.008, 260) : .white }
    var border: Color { scheme == .dark ? Self.oklch(0.35, 0.007, 260) : Color(hex: 0xe5e5e5) }
    var accent: Color { scheme == .dark ? Self.oklch(0.65, 0.2, 260) : Color(hex: 0x4577e9) }
    private static func oklch(_ l: Double, _ c: Double, _ degrees: Double) -> Color {
        let a = c * cos(degrees * .pi / 180), b = c * sin(degrees * .pi / 180)
        let x = pow(l + 0.3963377774 * a + 0.2158037573 * b, 3)
        let y = pow(l - 0.1055613458 * a - 0.0638541728 * b, 3)
        let z = pow(l - 0.0894841775 * a - 1.2914855480 * b, 3)
        func channel(_ value: Double) -> Double {
            let v = value <= 0.0031308 ? 12.92 * value : 1.055 * pow(value, 1 / 2.4) - 0.055
            return max(0, min(1, v))
        }
        return Color(.sRGB, red: channel(4.0767416621 * x - 3.3077115913 * y + 0.2309699292 * z),
                     green: channel(-1.2684380046 * x + 2.6097574011 * y - 0.3413193965 * z),
                     blue: channel(-0.0041960863 * x - 0.7034186147 * y + 1.7076147010 * z))
    }
}

private extension Color {
    init(hex: UInt32) {
        self.init(.sRGB, red: Double((hex >> 16) & 255) / 255, green: Double((hex >> 8) & 255) / 255, blue: Double(hex & 255) / 255)
    }
}

struct SettingsPanel<Content: View>: View {
    let title: String
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.system(size: 12, weight: .semibold))
            VStack(alignment: .leading, spacing: 14, content: content)
                .frame(maxWidth: .infinity, alignment: .leading).padding(16)
                .background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
        }
    }
}
