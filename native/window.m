#import <AppKit/AppKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <QuartzCore/QuartzCore.h>
#import <CoreImage/CoreImage.h>
#import <objc/runtime.h>
#include "slide_geometry.h"

static void migrateLegacyPreferences(NSUserDefaults *defaults, NSString *domain, NSDictionary *legacy) {
    NSDictionary *current = [defaults persistentDomainForName:domain] ?: @{};
    if ([current[@"migratedSunflowerPreferences"] boolValue]) return;
    for (NSString *key in @[@"autoZoom", @"windowFrame", @"fitWindowMaximum", @"defaultBannerDismissed"]) {
        if (!current[key] && legacy[key]) [defaults setObject:legacy[key] forKey:key];
    }
    [defaults setBool:YES forKey:@"migratedSunflowerPreferences"];
}

static NSUserDefaults *preferences(void) {
    static NSUserDefaults *defaults;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
#ifdef SF_TESTING
        defaults = [[NSUserDefaults alloc] initWithSuiteName:[NSString stringWithFormat:@"am.mkrtchyan.petalook.tests.%d", NSProcessInfo.processInfo.processIdentifier]];
#else
        defaults = [NSBundle.mainBundle.bundleIdentifier isEqualToString:@"am.mkrtchyan.petalook"]
            ? NSUserDefaults.standardUserDefaults : [[NSUserDefaults alloc] initWithSuiteName:@"am.mkrtchyan.petalook"];
        if (![defaults boolForKey:@"migratedSunflowerPreferences"]) {
            migrateLegacyPreferences(defaults, @"am.mkrtchyan.petalook",
                [defaults persistentDomainForName:@"app.sunflowerview.viewer"]);
        }
#endif
        [defaults registerDefaults:@{@"autoZoom": @1}];
    });
    return defaults;
}
int sf_preference(const char *key) { return (int)[preferences() integerForKey:@(key)]; }
void sf_set_preference(const char *key, int value) { [preferences() setInteger:value forKey:@(key)]; }

static NSRect savedFrame(void) { return NSRectFromString([preferences() stringForKey:@"windowFrame"] ?: @""); }
static BOOL validFrame(NSRect r) {
    return isfinite(r.origin.x) && isfinite(r.origin.y) && isfinite(r.size.width) && isfinite(r.size.height)
        && r.size.width >= 1 && r.size.height >= 1 && r.size.width < 20000 && r.size.height < 20000;
}
void sf_saved_size(double *w, double *h) {
    NSRect frame = savedFrame();
    *w = validFrame(frame) ? frame.size.width : 1000;
    *h = validFrame(frame) ? frame.size.height : 720;
}
void sf_fit_size(double *w, double *h) {
    NSSize maximum = NSSizeFromString([preferences() stringForKey:@"fitWindowMaximum"] ?: @"");
    if (validFrame((NSRect){NSZeroPoint, maximum})) {
        *w = maximum.width; *h = maximum.height;
    } else {
        sf_saved_size(w, h);
    }
}
void sf_save_fit_size(double w, double h) {
    NSSize maximum = NSMakeSize(w, h);
    if (validFrame((NSRect){NSZeroPoint, maximum}))
        [preferences() setObject:NSStringFromSize(maximum) forKey:@"fitWindowMaximum"];
}
static NSWindow *windowFor(void *view) { return ((__bridge NSView *)view).window; }
static NSRect available(NSWindow *window) { return NSInsetRect((window.screen ?: NSScreen.mainScreen).visibleFrame, 8, 8); }
static NSRect constrain(NSRect frame, NSRect screen) {
    frame.size.width = MIN(MAX(frame.size.width, 1), screen.size.width);
    frame.size.height = MIN(MAX(frame.size.height, 1), screen.size.height);
    frame.origin.x = MIN(MAX(frame.origin.x, screen.origin.x), NSMaxX(screen) - frame.size.width);
    frame.origin.y = MIN(MAX(frame.origin.y, screen.origin.y), NSMaxY(screen) - frame.size.height);
    return frame;
}

@interface SFSlide : NSObject
@property NSWindow *ghost;
@property NSWindow *incoming;
@property SFSlide *predecessor;
@property BOOL presented;
@property NSRect origin;
@property NSRect destination;
@property BOOL running;
@property BOOL restoreOrigin;
@property NSInteger direction;
@end
@implementation SFSlide
- (void)dealloc { [_ghost close]; [_incoming close]; }
@end
static const char slideKey;
static SFSlide *slideFor(NSWindow *window) { return objc_getAssociatedObject(window, &slideKey); }
static void closeSnapshots(SFSlide *slide) {
    while (slide) {
        SFSlide *previous = slide.predecessor;
        [slide.ghost close];
        [slide.incoming close];
        slide.ghost = nil;
        slide.incoming = nil;
        slide.predecessor = nil;
        slide = previous;
    }
}
int sf_window_transition_probe(void *view, double *values) {
    NSWindow *window = windowFor(view);
    SFSlide *slide = slideFor(window);
    if (!slide) return 0;
    NSRect a = slide.ghost.frame, b = slide.incoming.frame;
    values[0] = a.origin.x; values[1] = a.size.width; values[2] = slide.ghost.alphaValue;
    values[3] = b.origin.x; values[4] = b.size.width; values[5] = slide.incoming.alphaValue;
    NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(
        kCGWindowListOptionIncludingWindow, (CGWindowID)window.windowNumber));
    values[6] = windows.count ? [windows[0][(__bridge id)kCGWindowAlpha] doubleValue] : -1;
    return slide.incoming ? 2 : 1;
}
void sf_window_restore_origin(void *view) {
    NSWindow *window = windowFor(view);
    SFSlide *slide = slideFor(window);
    if (!slide) return;
    slide.restoreOrigin = YES;
    dispatch_async(dispatch_get_main_queue(), ^{
        if (slideFor(window) == slide) {
            slide.destination = slide.origin;
            [window setFrame:slide.origin display:YES];
        }
    });
}
void sf_window_finish_transition(void *view) {
    NSView *nativeView = (__bridge NSView *)view;
    NSWindow *window = windowFor(view);
    SFSlide *slide = slideFor(window);
    if (!slide) return;
    objc_setAssociatedObject(window, &slideKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    dispatch_async(dispatch_get_main_queue(), ^{
        SFSlide *replacement = slideFor(window);
        if (replacement && !replacement.presented) {
            SFSlide *tail = replacement;
            while (tail.predecessor) tail = tail.predecessor;
            tail.predecessor = slide;
            return;
        }
        if (!replacement) {
            if (slide.restoreOrigin) [window setFrame:slide.origin display:YES];
            // A transparent window stops GPUI's display link. Draw under the snapshots
            // through its layer delegate, which presents Metal inside this transaction.
            [nativeView.layer setNeedsDisplay];
            [nativeView.layer displayIfNeeded];
            [CATransaction flush];
            if (!slideFor(window)) window.alphaValue = 1;
        }
        closeSnapshots(slide);
        [CATransaction flush];
    });
}
void sf_window_complete_transition(void *view, uint64_t epoch) {
    NSWindow *window = windowFor(view);
    sf_window_finish_transition(view);
    dispatch_async(dispatch_get_main_queue(), ^{
        [NSNotificationCenter.defaultCenter postNotificationName:@"SFTransitionFinished" object:window
            userInfo:@{@"epoch": @(epoch)}];
    });
}
void sf_window_chrome(void *view, bool visible) {
    NSWindow *window = windowFor(view);
    window.titleVisibility = NSWindowTitleHidden;
    window.titlebarAppearsTransparent = YES;
    for (NSNumber *button in @[@(NSWindowCloseButton), @(NSWindowMiniaturizeButton), @(NSWindowZoomButton)])
        [window standardWindowButton:button.integerValue].hidden = !visible;
}
void sf_window_configure(void *view) {
    NSWindow *window = windowFor(view);
    window.styleMask &= ~NSWindowStyleMaskResizable;
    window.movable = NO;
    window.collectionBehavior |= NSWindowCollectionBehaviorFullScreenPrimary;
    window.titleVisibility = NSWindowTitleHidden;
    window.titlebarAppearsTransparent = YES;
    __weak NSWindow *weakWindow = window;
    [NSNotificationCenter.defaultCenter addObserverForName:NSWindowWillEnterFullScreenNotification object:window queue:nil usingBlock:^(__unused NSNotification *note) {
        weakWindow.styleMask |= NSWindowStyleMaskResizable;
    }];
    [NSNotificationCenter.defaultCenter addObserverForName:NSWindowDidExitFullScreenNotification object:window queue:nil usingBlock:^(__unused NSNotification *note) {
        weakWindow.styleMask &= ~NSWindowStyleMaskResizable;
    }];
    [NSNotificationCenter.defaultCenter addObserverForName:NSApplicationWillTerminateNotification object:nil queue:nil usingBlock:^(__unused NSNotification *note) {
        NSWindow *closing = weakWindow;
        if (closing && !(closing.styleMask & NSWindowStyleMaskFullScreen)) {
            SFSlide *slide = slideFor(closing);
            NSRect finalFrame = slide.running ? slide.destination : closing.frame;
            if (validFrame(finalFrame)) [preferences() setObject:NSStringFromRect(finalFrame) forKey:@"windowFrame"];
        }
    }];
    NSRect frame = savedFrame();
    if (validFrame(frame)) {
        NSScreen *screen = nil;
        for (NSScreen *candidate in NSScreen.screens) {
            if (NSIntersectsRect(candidate.visibleFrame, frame)) { screen = candidate; break; }
        }
        NSRect restored = constrain(frame, NSInsetRect((screen ?: window.screen ?: NSScreen.mainScreen).visibleFrame, 8, 8));
        dispatch_async(dispatch_get_main_queue(), ^{ [window setFrame:restored display:YES]; });
    }
}
void sf_window_save(void *view) {
    NSWindow *window = windowFor(view);
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!slideFor(window) && !(window.styleMask & NSWindowStyleMaskFullScreen) && validFrame(window.frame))
            [preferences() setObject:NSStringFromRect(window.frame) forKey:@"windowFrame"];
    });
}
void sf_window_toggle_fullscreen(void *view) {
    NSWindow *window = windowFor(view);
    dispatch_async(dispatch_get_main_queue(), ^{
        // AppKit requires this capability while changing fullscreen state.
        window.styleMask |= NSWindowStyleMaskResizable;
        [window toggleFullScreen:nil];
    });
}
void sf_window_available(void *view, double *w, double *h) {
    NSRect screen = available(windowFor(view));
    *w = screen.size.width; *h = screen.size.height;
}
void sf_window_resize(void *view, double width, double height) {
    NSWindow *window = windowFor(view);
    if (!isfinite(width) || !isfinite(height)) return;
    // GPUI owns the window during an update. Deliver AppKit resize callbacks after that borrow ends.
    dispatch_async(dispatch_get_main_queue(), ^{
        if (window.styleMask & NSWindowStyleMaskFullScreen) return;
        NSRect frame = window.frame;
        frame.origin.x += (frame.size.width - width) * 0.5;
        frame.origin.y += (frame.size.height - height) * 0.5;
        frame.size = NSMakeSize(width, height);
        [window setFrame:constrain(frame, available(window)) display:YES];
    });
}
static CGFloat magneticCoordinate(CGFloat proposed, CGFloat target, BOOL *snapped) {
    CGFloat distance = fabs(proposed - target);
    *snapped = distance <= (*snapped ? 24 : 12);
    return *snapped ? target : proposed;
}

static NSPoint pointerForEvent(NSEvent *event) {
    CGEventRef cgEvent = event.CGEvent;
    NSScreen *primary = NSScreen.screens.firstObject;
    if (cgEvent && primary) {
        CGPoint point = CGEventGetLocation(cgEvent);
        return NSMakePoint(point.x, NSMaxY(primary.frame) - point.y);
    }
    return [event.window convertPointToScreen:event.locationInWindow];
}

static const char dragKey;
@interface SFWindowDrag : NSObject
@property (weak) NSWindow *window;
@property NSPoint initialPointer;
@property NSRect initialFrame;
@property NSScreen *screen;
@property BOOL snappedX;
@property BOOL snappedY;
@property id monitor;
@property NSArray *observers;
- (void)moveTo:(NSPoint)pointer;
- (void)stop;
@end
@implementation SFWindowDrag
- (void)moveTo:(NSPoint)pointer {
    NSWindow *window = self.window;
    if (!window || window.styleMask & NSWindowStyleMaskFullScreen) { [self stop]; return; }
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    for (NSScreen *candidate in NSScreen.screens) {
        if (NSPointInRect(pointer, candidate.frame)) { screen = candidate; break; }
    }
    if (self.screen != screen) {
        self.screen = screen;
        self.snappedX = self.snappedY = NO;
    }
    NSRect frame = self.initialFrame;
    frame.origin.x += pointer.x - self.initialPointer.x;
    frame.origin.y += pointer.y - self.initialPointer.y;
    NSRect area = screen.visibleFrame;
    BOOL x = self.snappedX, y = self.snappedY;
    frame.origin.x = magneticCoordinate(frame.origin.x, NSMidX(area) - frame.size.width / 2, &x);
    frame.origin.y = magneticCoordinate(frame.origin.y, NSMidY(area) - frame.size.height / 2, &y);
    self.snappedX = x; self.snappedY = y;
    [window setFrameOrigin:frame.origin];
}
- (void)stop {
    if (self.monitor) [NSEvent removeMonitor:self.monitor];
    self.monitor = nil;
    for (id observer in self.observers) [NSNotificationCenter.defaultCenter removeObserver:observer];
    self.observers = nil;
    NSWindow *window = self.window;
    if (window && !(window.styleMask & NSWindowStyleMaskFullScreen) && validFrame(window.frame))
        [preferences() setObject:NSStringFromRect(window.frame) forKey:@"windowFrame"];
    if (objc_getAssociatedObject(window, &dragKey) == self)
        objc_setAssociatedObject(window, &dragKey, nil, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}
- (void)dealloc {
    if (_monitor) [NSEvent removeMonitor:_monitor];
    for (id observer in _observers) [NSNotificationCenter.defaultCenter removeObserver:observer];
}
@end

void sf_window_start_drag(void *view) {
    NSWindow *window = windowFor(view);
    if (!window || window.styleMask & NSWindowStyleMaskFullScreen || slideFor(window)) return;
    NSEvent *event = NSApp.currentEvent;
    if (event.type != NSEventTypeLeftMouseDown) return;
    SFWindowDrag *previous = objc_getAssociatedObject(window, &dragKey);
    [previous stop];
    SFWindowDrag *drag = [SFWindowDrag new];
    drag.window = window;
    // Use the press location; the live pointer may already be ahead in a queued drag.
    drag.initialPointer = pointerForEvent(event);
    drag.initialFrame = window.frame;
    __weak SFWindowDrag *weakDrag = drag;
    drag.monitor = [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskLeftMouseDragged | NSEventMaskLeftMouseUp
        handler:^NSEvent *(NSEvent *next) {
            SFWindowDrag *active = weakDrag;
            if (!active) return next;
            if (next.type == NSEventTypeLeftMouseUp) {
                [active stop];
                return next;
            }
            [active moveTo:pointerForEvent(next)];
            return nil;
        }];
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    drag.observers = @[
        [center addObserverForName:NSApplicationWillResignActiveNotification object:nil queue:nil
                       usingBlock:^(__unused NSNotification *note) { [weakDrag stop]; }],
        [center addObserverForName:NSWindowWillCloseNotification object:window queue:nil
                       usingBlock:^(__unused NSNotification *note) { [weakDrag stop]; }]
    ];
    objc_setAssociatedObject(window, &dragKey, drag, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
}

bool sf_reduce_motion(void) { return NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion; }

@interface SFSlideView : NSView
@property (weak) NSWindow *host;
@end
@implementation SFSlideView
- (BOOL)acceptsFirstMouse:(__unused NSEvent *)event { return YES; }
- (void)mouseDown:(__unused NSEvent *)event {
    NSWindow *host = self.host;
    if (!host) return;
    [NSApp activateIgnoringOtherApps:YES];
    [host makeKeyAndOrderFront:nil];
    [NSNotificationCenter.defaultCenter postNotificationName:@"SFTransitionClicked" object:host];
}
@end

int sf_window_snapshot_count(void *view) {
    NSWindow *host = windowFor(view);
    int count = 0;
    for (NSWindow *candidate in NSApp.windows) {
        if (candidate.visible && [candidate.contentView isKindOfClass:SFSlideView.class]
            && ((SFSlideView *)candidate.contentView).host == host) count++;
    }
    return count;
}

NSWindow *sf_gesture_host(NSWindow *window) {
    if ([window.contentView isKindOfClass:SFSlideView.class])
        return ((SFSlideView *)window.contentView).host;
    return window;
}

static CGImageRef createSlideBitmap(CGImageRef image, NSSize size, NSRect imageRect, CGFloat scale) {
    size_t width = (size_t)ceil(size.width * scale), height = (size_t)ceil(size.height * scale);
    if (!width || !height) return NULL;
    CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef context = CGBitmapContextCreate(NULL, width, height, 8, 0, color,
        kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGColorSpaceRelease(color);
    if (!context) return NULL;
    CGContextScaleCTM(context, width / size.width, height / size.height);
    CGRect bounds = CGRectMake(0, 0, size.width, size.height);
    CGContextClearRect(context, bounds);
    CGPathRef outline = CGPathCreateWithRoundedRect(bounds, 16, 16, NULL);
    CGContextAddPath(context, outline);
    CGContextClip(context);
    CGPathRelease(outline);
    CGContextSetRGBFillColor(context, 23./255, 24./255, 27./255, 1);
    CGContextFillRect(context, CGRectMake(0, 0, size.width, size.height));
    CGContextSetInterpolationQuality(context, kCGInterpolationHigh);
    CGContextDrawImage(context, CGRectMake(imageRect.origin.x,
        size.height - NSMaxY(imageRect), imageRect.size.width, imageRect.size.height), image);
    CGImageRef bitmap = CGBitmapContextCreateImage(context);
    CGContextRelease(context);
    return bitmap;
}

static void snapshotImage(NSWindow *window, CGImageRef cg, NSRect imageRect, int direction) {
    SFSlide *slide = direction ? [SFSlide new] : slideFor(window);
    if (!slide) return;
    CGImageRef bitmap = createSlideBitmap(cg, window.frame.size, imageRect, window.backingScaleFactor);
    if (!bitmap) return;
    if (!direction && slide.incoming) {
        // Refresh the existing surface without orphaning a visible snapshot window.
        slide.incoming.contentView.layer.contents = (__bridge id)bitmap;
        CGImageRelease(bitmap);
        return;
    }
    SFSlideView *content = [[SFSlideView alloc] initWithFrame:NSMakeRect(0, 0, window.frame.size.width, window.frame.size.height)];
    content.host = window;
    CALayer *layer = [CALayer layer];
    layer.contents = (__bridge id)bitmap;
    layer.contentsScale = window.backingScaleFactor;
    layer.contentsGravity = kCAGravityResize;
    layer.actions = @{ @"contents": NSNull.null, @"bounds": NSNull.null, @"position": NSNull.null };
    // A hosting layer owns a ready bitmap; AppKit never replaces it with a deferred drawRect.
    content.layer = layer;
    content.wantsLayer = YES;
    CGImageRelease(bitmap);
    NSWindow *snapshot = [[NSWindow alloc] initWithContentRect:window.frame styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
    snapshot.releasedWhenClosed = NO;
    snapshot.ignoresMouseEvents = NO;
    snapshot.hasShadow = YES;
    snapshot.animationBehavior = NSWindowAnimationBehaviorNone;
    snapshot.opaque = NO;
    snapshot.backgroundColor = NSColor.clearColor;
    snapshot.contentView = content;
    snapshot.level = window.level;
    snapshot.collectionBehavior = NSWindowCollectionBehaviorTransient | NSWindowCollectionBehaviorIgnoresCycle;
    if (!direction) {
        slide.destination = window.frame;
        slide.incoming = snapshot;
        snapshot.alphaValue = 0;
        return;
    }
    slide.origin = window.frame;
    slide.direction = direction;
    slide.ghost = snapshot;
    objc_setAssociatedObject(window, &slideKey, slide, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    // GPUI may render a cached target before the next main-queue turn.
    // Cover and hide the host before returning to the code that replaces its media.
    [slide.ghost orderWindow:NSWindowAbove relativeTo:window.windowNumber];
    window.alphaValue = 0;
    // Commit WindowServer opacity before GPUI can submit the incoming Metal frame.
    [CATransaction flush];
    slide.presented = YES;
    closeSnapshots(slide.predecessor);
    slide.predecessor = nil;
}
void sf_window_snapshot(void *view, const uint8_t *bytes, uint32_t width, uint32_t height,
                        double x, double y, double w, double h, int direction) {
    if (direction) sf_window_finish_transition(view);
    NSWindow *window = windowFor(view);
    if (!width || !height || sf_reduce_motion() || (window.styleMask & NSWindowStyleMaskFullScreen)) return;
    NSMutableData *data = [NSMutableData dataWithBytes:bytes length:(NSUInteger)width * height * 4];
    uint8_t *pixels = data.mutableBytes;
    for (NSUInteger i = 0; i < data.length; i += 4)
        for (int c = 0; c < 3; c++) pixels[i+c] = pixels[i+c] * pixels[i+3] / 255;
    CGDataProviderRef provider = CGDataProviderCreateWithCFData((__bridge CFDataRef)data);
    CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGImageRef cg = CGImageCreate(width, height, 8, 32, width * 4, color,
        kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little, provider, NULL, false, kCGRenderingIntentDefault);
    CGColorSpaceRelease(color); CGDataProviderRelease(provider);
    if (!cg) return;
    snapshotImage(window, cg, NSMakeRect(x, y, w, h), direction);
    CGImageRelease(cg);
}
void sf_window_snapshot_video(void *view, void *buffer, double x, double y, double w, double h, int direction) {
    if (direction) sf_window_finish_transition(view);
    NSWindow *window = windowFor(view);
    if (!buffer || sf_reduce_motion() || (window.styleMask & NSWindowStyleMaskFullScreen)) return;
    CIImage *image = [CIImage imageWithCVPixelBuffer:(CVPixelBufferRef)buffer];
    CIContext *context = [CIContext contextWithOptions:@{kCIContextCacheIntermediates: @NO}];
    CGImageRef cg = [context createCGImage:image fromRect:image.extent];
    if (cg) {
        snapshotImage(window, cg, NSMakeRect(x, y, w, h), direction);
        CGImageRelease(cg);
    }
}
bool sf_window_begin_transition(void *view) {
    NSWindow *window = windowFor(view);
    SFSlide *slide = slideFor(window);
    if (!slide.incoming) return false;
    dispatch_async(dispatch_get_main_queue(), ^{
        if (slideFor(window) != slide) return;
        slide.running = YES;
    });
    return true;
}
void sf_window_transition(void *view, double progress) {
    NSWindow *window = windowFor(view);
    SFSlide *slide = slideFor(window);
    if (!slide) return;
    dispatch_async(dispatch_get_main_queue(), ^{
    if (slideFor(window) != slide || !slide.running) return;
    double p = MIN(MAX(progress, 0), 1);
    NSRect a = slide.origin, b = slide.destination;
    SFSlideFrames frames = sf_slide_frames(
        (SFSlideRect){a.origin.x, a.origin.y, a.size.width, a.size.height},
        (SFSlideRect){b.origin.x, b.origin.y, b.size.width, b.size.height}, p, (int)slide.direction);
    SFSlideRect out = frames.outgoing, in = frames.incoming;
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    [slide.ghost setFrame:NSMakeRect(out.x, out.y, out.width, out.height) display:YES];
    [slide.incoming setFrame:NSMakeRect(in.x, in.y, in.width, in.height) display:YES];
    slide.ghost.alphaValue = 1 - p;
    slide.incoming.alphaValue = p;
    [CATransaction commit];
    [CATransaction flush];
    // The first visible frame already has the gesture's position, scale and opacity.
    if (!slide.incoming.visible) {
        [slide.incoming orderWindow:NSWindowAbove relativeTo:window.windowNumber];
        [CATransaction flush];
    }
    });
}

static NSArray<NSString *> *viewerTypes(void) {
    return @[@"public.jpeg", @"public.png", @"public.tiff", @"public.heic", @"public.heif",
        @"com.compuserve.gif", @"com.microsoft.bmp", @"org.webmproject.webp", @"public.avif",
        @"com.microsoft.ico", @"public.mpeg-4", @"com.apple.quicktime-movie", @"com.apple.m4v-video"];
}
static int defaultStatus;
static NSString *defaultError;
int sf_default_status(void) {
    if (defaultStatus == 2 || defaultStatus == 3) return defaultStatus;
    NSString *identifier = @"am.mkrtchyan.petalook";
    for (NSString *name in viewerTypes()) {
        UTType *type = [UTType typeWithIdentifier:name];
        if (!type) continue;
        NSURL *url = [NSWorkspace.sharedWorkspace URLForApplicationToOpenContentType:type];
        if (!url || ![[NSBundle bundleWithURL:url].bundleIdentifier isEqualToString:identifier]) return 0;
    }
    return 1;
}
const char *sf_default_error(void) { return defaultError.UTF8String ?: "macOS could not update the default viewer."; }
static void setNextType(NSUInteger index, NSURL *app) {
    if (index >= viewerTypes().count) { defaultStatus = 0; return; }
    UTType *type = [UTType typeWithIdentifier:viewerTypes()[index]];
    if (!type) { setNextType(index + 1, app); return; }
    [NSWorkspace.sharedWorkspace setDefaultApplicationAtURL:app toOpenContentType:type completionHandler:^(NSError *error) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (error) { defaultError = error.localizedDescription; defaultStatus = 3; }
            else setNextType(index + 1, app);
        });
    }];
}
void sf_make_default(void) {
    if (defaultStatus == 2) return;
    NSURL *app = NSBundle.mainBundle.bundleURL;
    if (![app.pathExtension isEqualToString:@"app"]) {
        defaultError = @"Open the bundled Petalook.app before setting it as your default viewer.";
        defaultStatus = 3;
        return;
    }
    defaultError = nil;
    defaultStatus = 2;
    setNextType(0, app);
}
void sf_zoom_menu(int mode) {
    NSMenu *view = [NSApp.mainMenu itemWithTitle:@"View"].submenu;
    NSMenu *zoom = [view itemWithTitle:@"Auto Zoom"].submenu;
    [zoom itemWithTitle:@"Fit to Window"].state = mode == 0 ? NSControlStateValueOn : NSControlStateValueOff;
    [zoom itemWithTitle:@"Fit to Screen"].state = mode == 1 ? NSControlStateValueOn : NSControlStateValueOff;
}
