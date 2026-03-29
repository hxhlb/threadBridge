import AppKit
import CoreGraphics
import Foundation

struct Options {
    let inputPath: String
    let outputPath: String
    let cornerRadius: CGFloat
}

func parseOptions() -> Options? {
    let args = CommandLine.arguments
    guard args.count == 3 || args.count == 4 else {
        fputs(
            "usage: apply_macos_icon_mask.swift <input-png> <output-png> [corner-radius]\n",
            stderr
        )
        return nil
    }

    let radius: CGFloat
    if args.count == 4 {
        guard let parsed = Double(args[3]), parsed > 0 else {
            fputs("corner-radius must be a positive number\n", stderr)
            return nil
        }
        radius = CGFloat(parsed)
    } else {
        radius = 232
    }

    return Options(inputPath: args[1], outputPath: args[2], cornerRadius: radius)
}

func loadBitmapRep(from image: NSImage) -> NSBitmapImageRep? {
    if let rep = image.representations.compactMap({ $0 as? NSBitmapImageRep }).first {
        return rep
    }
    guard
        let tiff = image.tiffRepresentation,
        let rep = NSBitmapImageRep(data: tiff)
    else {
        return nil
    }
    return rep
}

func applyMask(_ options: Options) throws {
    let inputURL = URL(fileURLWithPath: options.inputPath)
    guard let image = NSImage(contentsOf: inputURL) else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 1,
            userInfo: [NSLocalizedDescriptionKey: "failed to load image \(options.inputPath)"]
        )
    }
    guard let inputRep = loadBitmapRep(from: image) else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 2,
            userInfo: [NSLocalizedDescriptionKey: "failed to decode bitmap for \(options.inputPath)"]
        )
    }

    let width = inputRep.pixelsWide
    let height = inputRep.pixelsHigh
    guard width > 0, height > 0 else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 3,
            userInfo: [NSLocalizedDescriptionKey: "image has invalid dimensions"]
        )
    }

    let bytesPerRow = width * 4
    var pixels = [UInt8](repeating: 0, count: height * bytesPerRow)
    guard let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 4,
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
            domain: "threadBridge.apply_macos_icon_mask",
            code: 5,
            userInfo: [NSLocalizedDescriptionKey: "failed to create RGBA bitmap context"]
        )
    }
    guard let cgImage = inputRep.cgImage else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 6,
            userInfo: [NSLocalizedDescriptionKey: "failed to extract source CGImage"]
        )
    }

    let rect = CGRect(x: 0, y: 0, width: width, height: height)
    let clipPath = CGPath(
        roundedRect: rect,
        cornerWidth: options.cornerRadius,
        cornerHeight: options.cornerRadius,
        transform: nil
    )
    context.clear(rect)
    context.addPath(clipPath)
    context.clip()
    context.draw(cgImage, in: rect)

    guard let outputImage = context.makeImage() else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 7,
            userInfo: [NSLocalizedDescriptionKey: "failed to encode masked PNG"]
        )
    }
    let outputRep = NSBitmapImageRep(cgImage: outputImage)
    guard let pngData = outputRep.representation(using: .png, properties: [:]) else {
        throw NSError(
            domain: "threadBridge.apply_macos_icon_mask",
            code: 7,
            userInfo: [NSLocalizedDescriptionKey: "failed to encode masked PNG"]
        )
    }

    let outputURL = URL(fileURLWithPath: options.outputPath)
    try FileManager.default.createDirectory(
        at: outputURL.deletingLastPathComponent(),
        withIntermediateDirectories: true
    )
    try pngData.write(to: outputURL)
}

guard let options = parseOptions() else {
    exit(2)
}

do {
    try applyMask(options)
} catch {
    fputs("\(error)\n", stderr)
    exit(1)
}
