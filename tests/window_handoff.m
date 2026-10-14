#define SF_TESTING 1
#import "../native/window.m"
#import "../native/media.m"
#include <assert.h>

@interface SFDeferredLayer : CALayer
@property NSUInteger wantedFrame;
@property NSUInteger drawnFrame;
@end
@implementation SFDeferredLayer
- (void)display { self.drawnFrame = self.wantedFrame; }
@end

@interface SFHandoffWindow : NSWindow
@property NSUInteger unexpectedReveals;
@property BOOL requireFreshFrame;
@end
@implementation SFHandoffWindow
- (BOOL)canBecomeKeyWindow { return YES; }
- (void)setAlphaValue:(CGFloat)alpha {
    if (alpha > 0 && slideFor(self)) self.unexpectedReveals++;
    if (alpha > 0 && self.requireFreshFrame) {
        SFDeferredLayer *layer = (SFDeferredLayer *)self.contentView.layer;
        assert(layer.drawnFrame == layer.wantedFrame && "revealed an old live frame after hiding the renderer");
    }
    [super setAlphaValue:alpha];
}
@end

@interface SFTestScroll : NSEvent
@property NSWindow *testWindow;
@property NSEventPhase testPhase;
@end
@implementation SFTestScroll
- (NSWindow *)window { return self.testWindow; }
- (NSEventType)type { return NSEventTypeScrollWheel; }
- (NSEventPhase)phase { return self.testPhase; }
- (CGFloat)scrollingDeltaX { return -31; }
- (CGFloat)scrollingDeltaY { return 2; }
- (BOOL)hasPreciseScrollingDeltas { return YES; }
- (NSEventModifierFlags)modifierFlags { return NSEventModifierFlagCommand; }
- (NSPoint)locationInWindow { return NSMakePoint(900, 900); }
@end

typedef struct { int cancellations, scrolls, phase; } InputProbe;
static void checkGesture(void *context, int kind, double value) {
    InputProbe *probe = context;
    assert(kind == 3 && value == 0);
    probe->cancellations++;
}
static void checkScroll(void *context, double x, double y, int phase, unsigned flags) {
    InputProbe *probe = context;
    assert(x == -31 && y == 2 && flags == 3);
    probe->scrolls++;
    probe->phase = phase;
}

static void checkInputDelivery(NSWindow *window, NSWindow *snapshot) {
    InputProbe probe = {0};
    SFTestScroll *event = [SFTestScroll new];
    event.testWindow = window;
    assert(!NSPointInRect(event.locationInWindow, window.contentView.bounds));
    for (int phase = 0; phase < 3; phase++) {
        event.testPhase = phase == 0 ? NSEventPhaseBegan : phase == 1 ? NSEventPhaseChanged : NSEventPhaseEnded;
        assert(!handleGestureEvent(event, window, &probe, checkGesture, checkScroll));
        assert(probe.phase == phase && probe.scrolls == phase + 1);
    }
    event.testWindow = snapshot;
    assert(sf_gesture_host(snapshot) == window);
    assert(!handleGestureEvent(event, window, &probe, checkGesture, checkScroll));
    assert(probe.scrolls == 4 && probe.phase == 2);
    event.testPhase = NSEventPhaseCancelled;
    assert(!handleGestureEvent(event, window, &probe, checkGesture, checkScroll));
    assert(probe.cancellations == 1 && probe.scrolls == 4);
    event.testWindow = nil;
    assert(handleGestureEvent(event, window, &probe, checkGesture, checkScroll) == event);
    void *monitor = sf_install_gestures((__bridge void *)window.contentView, &probe, checkGesture, checkScroll);
    assert([snapshot.contentView acceptsFirstMouse:event]);
    [snapshot.contentView mouseDown:event];
    assert(probe.cancellations == 2);
    [NSNotificationCenter.defaultCenter postNotificationName:NSApplicationWillResignActiveNotification object:NSApp];
    assert(probe.cancellations == 3);
    [NSNotificationCenter.defaultCenter postNotificationName:NSWindowDidResignKeyNotification object:window];
    assert(probe.cancellations == 4);
    sf_remove_gestures(monitor);
    [NSNotificationCenter.defaultCenter postNotificationName:@"SFTransitionClicked" object:window];
    assert(probe.cancellations == 4);
}

static void checkPixel(CGImageRef image, CGFloat scale, int x, int y, uint8_t red, uint8_t green, uint8_t blue) {
    CFDataRef data = CGDataProviderCopyData(CGImageGetDataProvider(image));
    const uint8_t *pixel = CFDataGetBytePtr(data) + (size_t)(y * scale) * CGImageGetBytesPerRow(image) + (size_t)(x * scale) * 4;
    assert(pixel[0] == blue && pixel[1] == green && pixel[2] == red && pixel[3] == 255);
    CFRelease(data);
}

static void checkRoundedCorners(CGImageRef image) {
    CFDataRef data = CGDataProviderCopyData(CGImageGetDataProvider(image));
    const uint8_t *pixels = CFDataGetBytePtr(data);
    size_t width = CGImageGetWidth(image), height = CGImageGetHeight(image), row = CGImageGetBytesPerRow(image);
    for (int y = 0; y < 2; y++)
        for (int x = 0; x < 2; x++)
            assert(pixels[(y ? height - 1 : 0) * row + (x ? width - 1 : 0) * 4 + 3] == 0);
    assert(pixels[(width / 2) * 4 + 3] == 255);
    CFRelease(data);
}

static void drain(void) {
    NSDate *until = [NSDate dateWithTimeIntervalSinceNow:0.08];
    while (until.timeIntervalSinceNow > 0)
        [NSRunLoop.mainRunLoop runMode:NSDefaultRunLoopMode beforeDate:until];
}

static NSDictionary *compositorInfo(NSWindow *window) {
    NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(
        kCGWindowListOptionIncludingWindow, (CGWindowID)window.windowNumber));
    assert(windows.count == 1);
    return windows[0];
}

static double compositorAlpha(NSWindow *window) {
    return [compositorInfo(window)[(__bridge id)kCGWindowAlpha] doubleValue];
}

static void checkMagneticDragging(NSWindow *window) {
    BOOL snapped = NO;
    assert(magneticCoordinate(87, 100, &snapped) == 87 && !snapped);
    assert(magneticCoordinate(88, 100, &snapped) == 100 && snapped);
    assert(magneticCoordinate(123, 100, &snapped) == 100 && snapped);
    assert(magneticCoordinate(125, 100, &snapped) == 125 && !snapped);
    assert(magneticCoordinate(-1005, -1000, &snapped) == -1000 && snapped);

    [window orderOut:nil];
    NSRect area = NSScreen.mainScreen.visibleFrame;
    NSSize size = window.frame.size;
    NSPoint centered = NSMakePoint(NSMidX(area) - size.width / 2, NSMidY(area) - size.height / 2);
    [window setFrameOrigin:NSMakePoint(centered.x - 100, centered.y - 80)];
    SFWindowDrag *drag = [SFWindowDrag new];
    drag.window = window;
    drag.initialFrame = window.frame;
    drag.initialPointer = NSMakePoint(NSMidX(area), NSMidY(area));
    [drag moveTo:NSMakePoint(drag.initialPointer.x + 110, drag.initialPointer.y + 75)];
    assert(fabs(window.frame.origin.x - centered.x) <= .5);
    assert(fabs(window.frame.origin.y - centered.y) <= .5);
    [drag moveTo:NSMakePoint(drag.initialPointer.x + 140, drag.initialPointer.y + 90)];
    assert(fabs(window.frame.origin.x - centered.x - 40) <= .5);
    assert(fabs(window.frame.origin.y - centered.y) <= .5);
    assert(!drag.snappedX && drag.snappedY);
    assert(NSEqualSizes(window.frame.size, size));
    double width, height, afterWidth, afterHeight;
    sf_fit_size(&width, &height);
    [drag stop];
    sf_fit_size(&afterWidth, &afterHeight);
    assert(width == afterWidth && height == afterHeight);
    assert(NSEqualRects(savedFrame(), window.frame));
}

static NSWindow *observedHost;
static NSUInteger incomingPresentations;
@interface NSWindow (SFPresentationProbe)
- (void)sf_test_orderWindow:(NSWindowOrderingMode)place relativeTo:(NSInteger)otherWindow;
@end
@implementation NSWindow (SFPresentationProbe)
- (void)sf_test_orderWindow:(NSWindowOrderingMode)place relativeTo:(NSInteger)otherWindow {
    SFSlide *slide = slideFor(observedHost);
    BOOL presenting = place != NSWindowOut && self == slide.incoming;
    if (presenting) {
        assert(!NSIntersectsRect(self.frame, slide.ghost.frame));
        assert(self.animationBehavior == NSWindowAnimationBehaviorNone);
    }
    [self sf_test_orderWindow:place relativeTo:otherWindow];
    if (presenting) {
        incomingPresentations++;
    }
}
@end

static void checkPreferenceMigration(void) {
    NSString *domain = [NSString stringWithFormat:@"am.mkrtchyan.petalook.migrationtest.%d", NSProcessInfo.processInfo.processIdentifier];
    NSUserDefaults *defaults = [[NSUserDefaults alloc] initWithSuiteName:domain];
    [defaults removePersistentDomainForName:domain];
    [defaults registerDefaults:@{@"autoZoom": @1}];
    [defaults setObject:@"{{20, 40}, {300, 900}}" forKey:@"windowFrame"];
    NSDictionary *legacy = @{
        @"autoZoom": @0, @"windowFrame": @"{{0, 0}, {900, 600}}",
        @"fitWindowMaximum": @"{900, 600}", @"defaultBannerDismissed": @YES,
        @"SUFeedURL": @"https://example.com/old-feed.xml"
    };
    migrateLegacyPreferences(defaults, domain, legacy);
    assert([defaults integerForKey:@"autoZoom"] == 0);
    assert([[defaults stringForKey:@"windowFrame"] isEqualToString:@"{{20, 40}, {300, 900}}"]);
    assert([[defaults stringForKey:@"fitWindowMaximum"] isEqualToString:@"{900, 600}"]);
    assert([defaults boolForKey:@"defaultBannerDismissed"]);
    assert(![defaults objectForKey:@"SUFeedURL"]);
    [defaults removeObjectForKey:@"fitWindowMaximum"];
    [defaults setBool:NO forKey:@"defaultBannerDismissed"];
    migrateLegacyPreferences(defaults, domain, legacy);
    assert(![defaults objectForKey:@"fitWindowMaximum"]);
    assert(![defaults boolForKey:@"defaultBannerDismissed"]);
    [defaults removePersistentDomainForName:domain];
    migrateLegacyPreferences(defaults, domain, nil);
    assert([defaults integerForKey:@"autoZoom"] == 1);
    [defaults removePersistentDomainForName:domain];
}

int main(void) {
    @autoreleasepool {
        checkPreferenceMigration();
        [NSApplication sharedApplication];
        SFHandoffWindow *window = [[SFHandoffWindow alloc] initWithContentRect:NSMakeRect(-2000, 0, 320, 240)
            styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
        window.releasedWhenClosed = NO;
        observedHost = window;
        method_exchangeImplementations(
            class_getInstanceMethod(NSWindow.class, @selector(orderWindow:relativeTo:)),
            class_getInstanceMethod(NSWindow.class, @selector(sf_test_orderWindow:relativeTo:)));
        [window orderFront:nil];
        [CATransaction flush];
        NSView *view = window.contentView;
        const uint8_t pixels[] = {0, 0, 255, 255, 0, 255, 0, 255,
                                  255, 0, 0, 255, 0, 0, 0, 0};
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, 1);
        SFSlide *first = slideFor(window);
        assert(first);
        // A cached next image may render before the main queue runs again.
        assert(window.alphaValue == 0 && first.ghost.visible && first.presented);
        assert(!first.ghost.ignoresMouseEvents); // The visible image must not pass clicks to another app.
        // AppKit's model alpha can be zero while WindowServer still displays the host.
        assert(compositorAlpha(window) == 0);
        // The visible outgoing window must already own complete pixels.
        assert(first.ghost.contentView.layer.contents != nil);
        assert(NSEqualSizes(first.ghost.contentView.layer.bounds.size, first.ghost.contentView.bounds.size));
        CGImageRef bitmap = (__bridge CGImageRef)first.ghost.contentView.layer.contents;
        checkRoundedCorners(bitmap);
        CGFloat scale = window.backingScaleFactor;
        checkPixel(bitmap, scale, 75, 35, 255, 0, 0);
        checkPixel(bitmap, scale, 245, 35, 0, 255, 0);
        checkPixel(bitmap, scale, 75, 205, 0, 0, 255);
        checkPixel(bitmap, scale, 245, 205, 23, 24, 27);
        checkPixel(bitmap, scale, 30, 30, 23, 24, 27);
        drain();
        assert(window.alphaValue == 0 && first.ghost.visible);
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, 0);
        assert(first.incoming.contentView.layer.contents != nil);
        checkRoundedCorners((__bridge CGImageRef)first.incoming.contentView.layer.contents);
        assert(sf_window_begin_transition((__bridge void *)view));
        drain();
        assert(!first.incoming.visible); // No incoming window until its first pose is ready.
        sf_window_transition((__bridge void *)view, 0);
        drain();
        assert(NSMinX(first.incoming.frame) >= NSMaxX(first.ghost.frame) + 19);
        assert(compositorAlpha(first.incoming) == 0);
        sf_window_transition((__bridge void *)view, 0.5);
        drain();
        assert(first.incoming.contentView.layer.contents != nil);
        assert(fabs(first.ghost.alphaValue - 0.5) < .001);
        assert(fabs(compositorAlpha(first.incoming) - 0.5) < .001);
        assert(compositorAlpha(window) == 0);
        assert(NSEqualSizes(first.ghost.contentView.layer.bounds.size, first.ghost.contentView.bounds.size));
        NSWindow *existingIncoming = first.incoming;
        NSRect existingPose = existingIncoming.frame;
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, 0);
        assert(first.incoming == existingIncoming);
        assert(NSEqualRects(existingIncoming.frame, existingPose));
        assert(existingIncoming.alphaValue == 0.5);
        assert(sf_window_snapshot_count((__bridge void *)view) == 2);
        checkInputDelivery(window, first.ghost);
        sf_window_finish_transition((__bridge void *)view);
        // Start a replacement before the previous transition's queued cleanup runs.
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, -1);
        SFSlide *second = slideFor(window);
        assert(second && second != first);
        assert(second.ghost.contentView.layer.contents != nil);
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, 1);
        SFSlide *third = slideFor(window);
        drain();
        assert(window.alphaValue == 0 && third.ghost.visible);
        assert(window.unexpectedReveals == 0);
        assert(!first.ghost.visible && !first.incoming.visible);
        assert(!second.ghost.visible);
        sf_window_finish_transition((__bridge void *)view);
        drain();
        assert(window.alphaValue == 1 && !slideFor(window));
        assert(!third.ghost.visible);
        // A held swipe can already be partway across when its decoded preview arrives.
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, -1);
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 60, 20, 200, 200, 0);
        assert(sf_window_begin_transition((__bridge void *)view));
        sf_window_transition((__bridge void *)view, 0.35);
        drain();
        assert(incomingPresentations == 2);
        sf_window_finish_transition((__bridge void *)view);
        drain();
        sf_save_fit_size(900, 600);
        sf_window_resize((__bridge void *)view, 200, 600);
        drain();
        assert(window.frame.size.width == 200 && window.frame.size.height == 600);
        sf_window_save((__bridge void *)view);
        drain();
        double width, height;
        sf_saved_size(&width, &height);
        assert(width == 200 && height == 600);
        sf_fit_size(&width, &height);
        assert(width == 900 && height == 600);
        sf_window_resize((__bridge void *)view, 900, 300);
        drain();
        assert(window.frame.size.width == 900 && window.frame.size.height == 300);
        sf_window_configure((__bridge void *)view);
        drain();
        assert(!(window.styleMask & NSWindowStyleMaskResizable) && !window.movable);
        [NSNotificationCenter.defaultCenter postNotificationName:NSWindowWillEnterFullScreenNotification object:window];
        assert(window.styleMask & NSWindowStyleMaskResizable);
        [NSNotificationCenter.defaultCenter postNotificationName:NSWindowDidExitFullScreenNotification object:window];
        assert(!(window.styleMask & NSWindowStyleMaskResizable));
        sf_window_resize((__bridge void *)view, 450, 300);
        drain();
        assert(window.frame.size.width == 450 && window.frame.size.height == 300);
        checkMagneticDragging(window);
        SFDeferredLayer *liveLayer = [SFDeferredLayer layer];
        view.layer = liveLayer;
        view.wantsLayer = YES;
        sf_window_snapshot((__bridge void *)view, pixels, 2, 2, 0, 0, 450, 300, 1);
        liveLayer.wantedFrame = 2;
        window.requireFreshFrame = YES;
        __block BOOL acknowledged = NO;
        id completionObserver = [NSNotificationCenter.defaultCenter addObserverForName:@"SFTransitionFinished"
            object:window queue:nil usingBlock:^(NSNotification *note) {
                assert([note.userInfo[@"epoch"] unsignedLongLongValue] == 42);
                assert(liveLayer.drawnFrame == 2 && window.alphaValue == 1);
                assert(sf_window_snapshot_count((__bridge void *)view) == 0);
                acknowledged = YES;
            }];
        sf_window_complete_transition((__bridge void *)view, 42);
        assert(!acknowledged);
        drain();
        assert(liveLayer.drawnFrame == 2 && acknowledged);
        [NSNotificationCenter.defaultCenter removeObserver:completionObserver];
        window.requireFreshFrame = NO;
        [window close];
        observedHost = nil;
        [preferences() removePersistentDomainForName:[NSString stringWithFormat:@"am.mkrtchyan.petalook.tests.%d", NSProcessInfo.processInfo.processIdentifier]];
        puts("Window handoff: magnetic dragging, fixed edges, input delivery, rounded pixels, compositor and remembered maximum passed.");
    }
}
