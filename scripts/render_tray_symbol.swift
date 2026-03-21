import AppKit
import CoreGraphics
import Foundation

struct Options {
    let symbolName: String
    let canvasSize: Int
    let pointSize: CGFloat
    let outputPath: String
}

func parseOptions() -> Options? {
    let args = CommandLine.arguments
    guard args.count == 5 else {
        fputs(
            "usage: render_tray_symbol.swift <symbol-name> <canvas-size> <point-size> <output-path>\n",
            stderr
        )
        return nil
    }

    guard let canvasSize = Int(args[2]), canvasSize > 0 else {
        fputs("canvas-size must be a positive integer\n", stderr)
        return nil
    }
    guard let pointValue = Double(args[3]), pointValue > 0 else {
        fputs("point-size must be a positive number\n", stderr)
        return nil
    }

    return Options(
        symbolName: args[1],
        canvasSize: canvasSize,
        pointSize: CGFloat(pointValue),
        outputPath: args[4]
    )
}

func renderSymbol(_ options: Options) throws {
    guard let baseImage = NSImage(
        systemSymbolName: options.symbolName,
        accessibilityDescription: nil
    ) else {
        throw NSError(
            domain: "threadBridge.render_tray_symbol",
            code: 1,
            userInfo: [NSLocalizedDescriptionKey: "missing SF Symbol \(options.symbolName)"]
        )
    }

    let configuration = NSImage.SymbolConfiguration(
        pointSize: options.pointSize,
        weight: .regular,
        scale: .medium
    )
    let image = (baseImage.withSymbolConfiguration(configuration) ?? baseImage)
    image.isTemplate = true

    let width = options.canvasSize
    let height = options.canvasSize
    let bytesPerRow = width * 4
    var pixels = [UInt8](repeating: 0, count: width * height * 4)

    guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) else {
        throw NSError(
            domain: "threadBridge.render_tray_symbol",
            code: 2,
            userInfo: [NSLocalizedDescriptionKey: "failed to create sRGB color space"]
        )
    }

    guard let context = CGContext(
        data: &pixels,
        width: width,
        height: height,
        bitsPerComponent: 8,
        bytesPerRow: bytesPerRow,
        space: colorSpace,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else {
        throw NSError(
            domain: "threadBridge.render_tray_symbol",
            code: 3,
            userInfo: [NSLocalizedDescriptionKey: "failed to create bitmap context"]
        )
    }

    let rect = NSRect(x: 0, y: 0, width: width, height: height)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
    NSColor.clear.setFill()
    rect.fill()
    image.draw(in: rect)
    NSGraphicsContext.restoreGraphicsState()

    let outputUrl = URL(fileURLWithPath: options.outputPath)
    try FileManager.default.createDirectory(
        at: outputUrl.deletingLastPathComponent(),
        withIntermediateDirectories: true
    )
    try Data(pixels).write(to: outputUrl)
}

guard let options = parseOptions() else {
    exit(2)
}

do {
    try renderSymbol(options)
} catch {
    fputs("\(error)\n", stderr)
    exit(1)
}
