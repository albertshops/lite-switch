import CoreGraphics

public enum WindowGeometry {
    public static func frame(
        for action: WindowAction,
        currentFrame: CGRect,
        sourceVisibleFrame: CGRect,
        destinationVisibleFrame: CGRect? = nil
    ) -> CGRect? {
        switch action {
        case .fillScreen:
            return sourceVisibleFrame
        case .moveLeft:
            return CGRect(
                x: sourceVisibleFrame.minX,
                y: sourceVisibleFrame.minY,
                width: sourceVisibleFrame.width / 2,
                height: sourceVisibleFrame.height
            )
        case .moveRight:
            return CGRect(
                x: sourceVisibleFrame.midX,
                y: sourceVisibleFrame.minY,
                width: sourceVisibleFrame.width / 2,
                height: sourceVisibleFrame.height
            )
        case .moveTop:
            return CGRect(
                x: sourceVisibleFrame.minX,
                y: sourceVisibleFrame.midY,
                width: sourceVisibleFrame.width,
                height: sourceVisibleFrame.height / 2
            )
        case .moveBottom:
            return CGRect(
                x: sourceVisibleFrame.minX,
                y: sourceVisibleFrame.minY,
                width: sourceVisibleFrame.width,
                height: sourceVisibleFrame.height / 2
            )
        case .switchScreen:
            guard let destinationVisibleFrame else { return nil }
            return transferredFrame(
                currentFrame,
                from: sourceVisibleFrame,
                to: destinationVisibleFrame
            )
        }
    }

    public static func appKitFrame(fromAccessibilityFrame frame: CGRect, desktopTop: CGFloat) -> CGRect {
        CGRect(x: frame.minX, y: desktopTop - frame.maxY, width: frame.width, height: frame.height)
    }

    public static func accessibilityFrame(fromAppKitFrame frame: CGRect, desktopTop: CGFloat) -> CGRect {
        CGRect(x: frame.minX, y: desktopTop - frame.maxY, width: frame.width, height: frame.height)
    }

    private static func transferredFrame(_ frame: CGRect, from source: CGRect, to destination: CGRect) -> CGRect {
        let size = CGSize(
            width: min(frame.width, destination.width),
            height: min(frame.height, destination.height)
        )
        let sourceTravel = CGSize(
            width: max(source.width - frame.width, 0),
            height: max(source.height - frame.height, 0)
        )
        let destinationTravel = CGSize(
            width: max(destination.width - size.width, 0),
            height: max(destination.height - size.height, 0)
        )
        let xRatio = sourceTravel.width > 0
            ? min(max((frame.minX - source.minX) / sourceTravel.width, 0), 1)
            : 0
        let yRatio = sourceTravel.height > 0
            ? min(max((frame.minY - source.minY) / sourceTravel.height, 0), 1)
            : 0

        return CGRect(
            x: destination.minX + destinationTravel.width * xRatio,
            y: destination.minY + destinationTravel.height * yRatio,
            width: size.width,
            height: size.height
        )
    }
}
