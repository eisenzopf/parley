// Independent macOS Vision check of the exported QR pixels.
import Foundation
import Vision
let arguments = CommandLine.arguments
guard arguments.count == 3 else { fatalError("Usage: swift e2e/decode-conference-qr.swift image.png expected-url") }
let request = VNDetectBarcodesRequest()
request.symbologies = [.qr]
try VNImageRequestHandler(url: URL(fileURLWithPath: arguments[1])).perform([request])
let values = request.results?.compactMap { $0.payloadStringValue } ?? []
guard values == [arguments[2]] else { fatalError("QR decoded to an unexpected destination") }
print("QR independently decoded to \(values[0])")
