import SwiftUI

/// Liquid Glass helpers, centralised so the whole app shares one definition of
/// "a glass surface" and any future API tweak lives in a single place.
extension View {
    /// Floats the view on a regular Liquid Glass surface clipped to `shape`.
    func glassSurface(in shape: some Shape = .capsule) -> some View {
        glassEffect(.regular, in: shape)
    }

    /// Glass surface tinted toward the accent colour, used for emphasis.
    func glassSurface(tinted color: Color, in shape: some Shape = .capsule) -> some View {
        glassEffect(.regular.tint(color), in: shape)
    }

    /// Interactive glass that reacts to touch, for tappable floating controls.
    func interactiveGlass(in shape: some Shape = .capsule) -> some View {
        glassEffect(.regular.interactive(), in: shape)
    }
}

/// A circular icon button rendered on interactive Liquid Glass. Used for the
/// composer's send/stop control and other floating affordances.
struct GlassIconButton: View {
    let systemName: String
    var prominent: Bool = false
    var tint: Color = .accentColor
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 17, weight: .semibold))
                .frame(width: 36, height: 36)
                .contentShape(.circle)
                .foregroundStyle(prominent ? AnyShapeStyle(.white) : AnyShapeStyle(.tint))
        }
        .buttonStyle(.plain)
        .glassEffect(
            prominent ? .regular.tint(tint).interactive() : .regular.interactive(),
            in: .circle
        )
    }
}
