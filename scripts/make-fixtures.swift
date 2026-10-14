import AppKit
import AVFoundation

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let portrait = CommandLine.arguments.contains("--portrait")
let output = directory.appendingPathComponent(portrait ? "clip11.mp4" : "clip10.mp4")
if FileManager.default.fileExists(atPath: output.path) { try FileManager.default.removeItem(at: output) }
let writer = try AVAssetWriter(outputURL: output, fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 960, AVVideoHeightKey: 540])
if portrait { input.transform = CGAffineTransform(a: 0, b: 1, c: -1, d: 0, tx: 540, ty: 0) }
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB, kCVPixelBufferWidthKey as String: 960, kCVPixelBufferHeightKey as String: 540])
writer.add(input)
writer.startWriting()
writer.startSession(atSourceTime: .zero)
for frame in 0..<360 {
    while !input.isReadyForMoreMediaData { Thread.sleep(forTimeInterval: 0.002) }
    var optional: CVPixelBuffer?
    CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &optional)
    let buffer = optional!
    CVPixelBufferLockBaseAddress(buffer, [])
    let context = CGContext(data: CVPixelBufferGetBaseAddress(buffer), width: 960, height: 540, bitsPerComponent: 8, bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue)!
    context.setFillColor(NSColor(calibratedRed: 0.07, green: 0.15, blue: 0.2, alpha: 1).cgColor)
    context.fill(CGRect(x: 0, y: 0, width: 960, height: 540))
    context.setFillColor(NSColor.systemYellow.cgColor)
    context.fillEllipse(in: CGRect(x: 100 + frame * 2, y: 200, width: 100, height: 100))
    context.setFillColor(NSColor.systemTeal.cgColor)
    context.fill(CGRect(x: 80, y: 70, width: frame * 2, height: 18))
    CVPixelBufferUnlockBaseAddress(buffer, [])
    adaptor.append(buffer, withPresentationTime: CMTime(value: Int64(frame), timescale: 30))
}
input.markAsFinished()
let semaphore = DispatchSemaphore(value: 0)
writer.finishWriting { semaphore.signal() }
semaphore.wait()
if writer.status != .completed { fatalError(writer.error?.localizedDescription ?? "Video fixture failed") }
print(output.path)
