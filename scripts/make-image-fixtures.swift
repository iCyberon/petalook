import Foundation
import ImageIO
import UniformTypeIdentifiers

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let width = 6000, height = 4000
var pixels = [UInt8](repeating: 255, count: width * height * 4)
// Deterministic textured pixels exercise actual decoding, not just solid fills.
var random: UInt32 = 42
for y in 0..<height {
    for x in 0..<width {
        random = random &* 1664525 &+ 1013904223
        let i = (y * width + x) * 4
        let noise = Int(random >> 27)
        pixels[i] = UInt8((x / 24 + noise) % 256)
        pixels[i + 1] = UInt8((y / 16 + noise) % 256)
        pixels[i + 2] = UInt8(((x + y) / 40 + noise) % 256)
    }
}
let provider = CGDataProvider(data: Data(pixels) as CFData)!
let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                    bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                    provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)!
for (ext, type) in [("jpg", UTType.jpeg), ("png", UTType.png), ("heic", UTType.heic)] {
    let url = directory.appendingPathComponent("large.\(ext)")
    let destination = CGImageDestinationCreateWithURL(url as CFURL, type.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(destination, image, [kCGImageDestinationLossyCompressionQuality: 0.88] as CFDictionary)
    precondition(CGImageDestinationFinalize(destination))
    print(url.path)
}
