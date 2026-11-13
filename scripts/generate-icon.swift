#!/usr/bin/env swift
import AppKit

// Master macOS App Icon Generator for Ferrite DM
// Generates high-resolution App Icon PNG and compiles into multi-size Ferrite.icns

let size: CGFloat = 1024
let image = NSImage(size: NSSize(width: size, height: size))
image.lockFocus()

guard let context = NSGraphicsContext.current?.cgContext else {
    fatalError("Failed to obtain CGContext")
}

// macOS Big Sur+ Squircle bounds
let inset: CGFloat = 100
let squircleRect = NSRect(x: inset, y: inset, width: size - (inset * 2), height: size - (inset * 2))
let cornerRadius: CGFloat = 185

// Background Shadow
context.saveGState()
let shadowColor = NSColor.black.withAlphaComponent(0.35).cgColor
context.setShadow(offset: CGSize(width: 0, height: -18), blur: 36, color: shadowColor)
let shadowPath = NSBezierPath(roundedRect: squircleRect, xRadius: cornerRadius, yRadius: cornerRadius)
NSColor.black.setFill()
shadowPath.fill()
context.restoreGState()

// Squircle clipping & background gradient
let squirclePath = NSBezierPath(roundedRect: squircleRect, xRadius: cornerRadius, yRadius: cornerRadius)
squirclePath.addClip()

// Primary gradient: Deep cobalt blue to bright cyan-blue (FDM modern aesthetic)
let grad = NSGradient(
    colorsAndLocations:
    (NSColor(red: 0.12, green: 0.38, blue: 0.88, alpha: 1.0), 0.0), // #1F61E0
    (NSColor(red: 0.08, green: 0.48, blue: 0.96, alpha: 1.0), 0.5), // #147BF5
    (NSColor(red: 0.05, green: 0.65, blue: 0.98, alpha: 1.0), 1.0)  // #0DA6FA
)!
grad.draw(in: squircleRect, angle: 90)

// Subtle top highlight sheen
context.saveGState()
let sheenGradient = NSGradient(
    starting: NSColor.white.withAlphaComponent(0.20),
    ending: NSColor.white.withAlphaComponent(0.0)
)!
let topRect = NSRect(x: inset, y: size - inset - 280, width: size - (inset * 2), height: 280)
sheenGradient.draw(in: topRect, angle: 90)
context.restoreGState()

// Draw the Ferrite DM Download Disc & Arrow Symbol
// We render the scaled icon path in crisp white with subtle inner depth
context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: -6), blur: 14, color: NSColor.black.withAlphaComponent(0.25).cgColor)

// Center symbol area
let symbolBoxSize: CGFloat = 460
let symbolX = (size - symbolBoxSize) / 2
let symbolY = (size - symbolBoxSize) / 2 - 10
let scale = symbolBoxSize / 24.0

// Translate & flip coordinates to match SVG coordinate system
context.translateBy(x: symbolX, y: symbolY + symbolBoxSize)
context.scaleBy(x: scale, y: -scale)

// SVG Path: M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 5v6.59l2.3-2.3a1 1 0 1 1 1.4 1.42l-4 4a1 1 0 0 1-1.4 0l-4-4a1 1 0 0 1 1.4-1.42l2.3 2.3V7a1 1 0 1 1 2 0zM7 17.5a1 1 0 0 1 1-1h8a1 1 0 1 1 0 2H8a1 1 0 0 1-1-1z
// We construct NSBezierPath with evenOdd rule
let path = NSBezierPath()
path.windingRule = .evenOdd

// Subpath 1: Outer circle
path.move(to: NSPoint(x: 12, y: 2))
path.curve(to: NSPoint(x: 2, y: 12), controlPoint1: NSPoint(x: 6.48, y: 2), controlPoint2: NSPoint(x: 2, y: 6.48))
path.curve(to: NSPoint(x: 12, y: 22), controlPoint1: NSPoint(x: 2, y: 17.52), controlPoint2: NSPoint(x: 6.48, y: 22))
path.curve(to: NSPoint(x: 22, y: 12), controlPoint1: NSPoint(x: 17.52, y: 22), controlPoint2: NSPoint(x: 22, y: 17.52))
path.curve(to: NSPoint(x: 12, y: 2), controlPoint1: NSPoint(x: 22, y: 6.48), controlPoint2: NSPoint(x: 17.52, y: 2))
path.close()

// Subpath 2: Downward arrow
path.move(to: NSPoint(x: 13, y: 7))
path.line(to: NSPoint(x: 13, y: 13.59))
path.line(to: NSPoint(x: 15.3, y: 11.29))
// arc/curve approx for rounded cap:
path.line(to: NSPoint(x: 16.7, y: 12.71))
path.line(to: NSPoint(x: 12.7, y: 16.71))
path.curve(to: NSPoint(x: 11.3, y: 16.71), controlPoint1: NSPoint(x: 12.31, y: 17.1), controlPoint2: NSPoint(x: 11.69, y: 17.1))
path.line(to: NSPoint(x: 7.3, y: 12.71))
path.line(to: NSPoint(x: 8.7, y: 11.29))
path.line(to: NSPoint(x: 11, y: 13.59))
path.line(to: NSPoint(x: 11, y: 7))
path.curve(to: NSPoint(x: 13, y: 7), controlPoint1: NSPoint(x: 11, y: 6.45), controlPoint2: NSPoint(x: 13, y: 6.45))
path.close()

// Subpath 3: Bottom tray bar
path.move(to: NSPoint(x: 8, y: 16.5))
path.line(to: NSPoint(x: 16, y: 16.5))
path.curve(to: NSPoint(x: 16, y: 18.5), controlPoint1: NSPoint(x: 16.55, y: 16.5), controlPoint2: NSPoint(x: 16.55, y: 18.5))
path.line(to: NSPoint(x: 8, y: 18.5))
path.curve(to: NSPoint(x: 8, y: 16.5), controlPoint1: NSPoint(x: 7.45, y: 18.5), controlPoint2: NSPoint(x: 7.45, y: 16.5))
path.close()

NSColor.white.setFill()
path.fill()

context.restoreGState()
image.unlockFocus()

// Output paths
let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "resources"
let masterPng = "\(outDir)/Ferrite-1024.png"
let iconsetDir = "\(outDir)/Ferrite.iconset"
let icnsPath = "\(outDir)/Ferrite.icns"

guard let tiff = image.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff),
      let png = rep.representation(using: .png, properties: [:]) else {
    fatalError("Failed to convert image to PNG")
}

try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
try png.write(to: URL(fileURLWithPath: masterPng))
print("Wrote master 1024x1024 icon to \(masterPng)")

// Create iconset directory
try? FileManager.default.createDirectory(atPath: iconsetDir, withIntermediateDirectories: true)

let sizes: [(String, Int)] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024)
]

for (name, px) in sizes {
    let resized = NSImage(size: NSSize(width: px, height: px))
    resized.lockFocus()
    image.draw(in: NSRect(x: 0, y: 0, width: px, height: px),
               from: NSRect(x: 0, y: 0, width: size, height: size),
               operation: .copy,
               fraction: 1.0)
    resized.unlockFocus()
    if let rTiff = resized.tiffRepresentation,
       let rRep = NSBitmapImageRep(data: rTiff),
       let rPng = rRep.representation(using: .png, properties: [:]) {
        try? rPng.write(to: URL(fileURLWithPath: "\(iconsetDir)/\(name)"))
    }
}
print("Generated iconset at \(iconsetDir)")

// Run iconutil to create .icns
let task = Process()
task.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
task.arguments = ["-c", "icns", iconsetDir, "-o", icnsPath]
try? task.run()
task.waitUntilExit()

if task.terminationStatus == 0 {
    print("Successfully built \(icnsPath)")
} else {
    print("Failed to run iconutil")
}
