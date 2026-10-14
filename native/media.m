#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <ImageIO/ImageIO.h>
#include <pthread/qos.h>

void sf_image_worker_priority(bool prefetch) {
    pthread_set_qos_class_self_np(prefetch ? QOS_CLASS_UTILITY : QOS_CLASS_USER_INITIATED, 0);
}

typedef struct {
    uint8_t *data;
    uint32_t width, height, original_width, original_height;
} SFImage;

bool sf_decode_image(const char *path, uint32_t limit, SFImage *out) {
    @autoreleasepool {
        NSURL *url = [NSURL fileURLWithPath:@(path)];
        CGImageSourceRef source = CGImageSourceCreateWithURL((__bridge CFURLRef)url, NULL);
        if (!source) return false;
        NSDictionary *properties = CFBridgingRelease(CGImageSourceCopyPropertiesAtIndex(source, 0, NULL));
        uint32_t ow = [properties[(__bridge NSString *)kCGImagePropertyPixelWidth] unsignedIntValue];
        uint32_t oh = [properties[(__bridge NSString *)kCGImagePropertyPixelHeight] unsignedIntValue];
        int orientation = [properties[(__bridge NSString *)kCGImagePropertyOrientation] intValue];
        if (orientation >= 5 && orientation <= 8) { uint32_t t = ow; ow = oh; oh = t; }
        NSDictionary *options = @{
            (__bridge NSString *)kCGImageSourceCreateThumbnailFromImageAlways: @YES,
            (__bridge NSString *)kCGImageSourceCreateThumbnailWithTransform: @YES,
            (__bridge NSString *)kCGImageSourceShouldCacheImmediately: @YES,
            (__bridge NSString *)kCGImageSourceThumbnailMaxPixelSize: @(limit)
        };
        CGImageRef image = CGImageSourceCreateThumbnailAtIndex(source, 0, (__bridge CFDictionaryRef)options);
        CFRelease(source);
        if (!image) return false;
        size_t w = CGImageGetWidth(image), h = CGImageGetHeight(image);
        if (!w || !h || w > limit || h > limit) { CGImageRelease(image); return false; }
        uint8_t *bytes = calloc(w * h, 4);
        if (!bytes) { CGImageRelease(image); return false; }
        CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        CGContextRef context = CGBitmapContextCreate(bytes, w, h, 8, w * 4, color,
            kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
        CGColorSpaceRelease(color);
        if (!context) { free(bytes); CGImageRelease(image); return false; }
        CGContextDrawImage(context, CGRectMake(0, 0, w, h), image);
        CGContextRelease(context); CGImageRelease(image);
        // GPUI's image shader expects straight-alpha BGRA.
        for (size_t i = 0; i < w * h * 4; i += 4) {
            unsigned a = bytes[i + 3];
            if (a && a < 255) for (int c = 0; c < 3; ++c) bytes[i + c] = MIN(255, bytes[i + c] * 255 / a);
        }
        *out = (SFImage){ bytes, (uint32_t)w, (uint32_t)h, ow, oh };
        return true;
    }
}
void sf_free_image(uint8_t *data) { free(data); }

@interface SFVideo : NSObject
@property AVPlayer *player;
@property AVPlayerItemVideoOutput *output;
@property BOOL closed;
@property BOOL wantsPlaying;
@property BOOL failed;
@end
@implementation SFVideo
@end

void *sf_video_open(const char *path) {
    SFVideo *video = [SFVideo new];
    AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:@(path)] options:nil];
    video.output = [[AVPlayerItemVideoOutput alloc] initWithPixelBufferAttributes:@{
        (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange),
        (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
        (id)kCVPixelBufferMetalCompatibilityKey: @YES
    }];
    video.player = [AVPlayer new];
    video.wantsPlaying = YES;
    video.player.actionAtItemEnd = AVPlayerActionAtItemEndPause;
    [asset loadValuesAsynchronouslyForKeys:@[@"tracks"] completionHandler:^{
        @autoreleasepool {
            AVAssetTrack *track = [asset tracksWithMediaType:AVMediaTypeVideo].firstObject;
            AVVideoComposition *composition = nil;
            if (track && !CGAffineTransformIsIdentity(track.preferredTransform)) {
                composition = [AVVideoComposition videoCompositionWithPropertiesOfAsset:asset];
            }
            dispatch_async(dispatch_get_main_queue(), ^{
                if (video.closed) return;
                if (!track) { video.failed = YES; return; }
                AVPlayerItem *item = [AVPlayerItem playerItemWithAsset:asset];
                item.videoComposition = composition;
                [item addOutput:video.output];
                [video.player replaceCurrentItemWithPlayerItem:item];
                if (video.wantsPlaying) [video.player play];
            });
        }
    }];
    return (__bridge_retained void *)video;
}
void sf_video_close(void *handle) {
    SFVideo *video = CFBridgingRelease(handle);
    video.closed = YES;
    [video.player pause];
    [video.player replaceCurrentItemWithPlayerItem:nil];
}
void *sf_video_frame(void *handle) {
    SFVideo *video = (__bridge SFVideo *)handle;
    CMTime time = video.player.currentTime;
    if (![video.output hasNewPixelBufferForItemTime:time]) return NULL;
    return [video.output copyPixelBufferForItemTime:time itemTimeForDisplay:NULL];
}
double sf_video_time(void *handle) { return CMTimeGetSeconds(((__bridge SFVideo *)handle).player.currentTime); }
double sf_video_duration(void *handle) { return CMTimeGetSeconds(((__bridge SFVideo *)handle).player.currentItem.duration); }
int sf_video_status(void *handle) {
    SFVideo *video = (__bridge SFVideo *)handle;
    return video.failed ? 2 : (int)video.player.currentItem.status;
}
bool sf_video_playing(void *handle) { return ((__bridge SFVideo *)handle).player.rate != 0; }
void sf_video_play(void *handle, bool playing) {
    SFVideo *video = (__bridge SFVideo *)handle;
    video.wantsPlaying = playing;
    AVPlayer *player = video.player;
    if (playing) {
        double duration = CMTimeGetSeconds(player.currentItem.duration);
        if (isfinite(duration) && CMTimeGetSeconds(player.currentTime) >= duration - 0.05) [player seekToTime:kCMTimeZero];
        [player play];
    } else [player pause];
}
void sf_video_seek(void *handle, double seconds) {
    [((__bridge SFVideo *)handle).player seekToTime:CMTimeMakeWithSeconds(seconds, 600)
        toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero];
}
void sf_video_mute(void *handle, bool muted) { ((__bridge SFVideo *)handle).player.muted = muted; }

typedef void (*SFGestureCallback)(void *, int, double);
typedef void (*SFScrollCallback)(void *, double, double, int, unsigned);
extern NSWindow *sf_gesture_host(NSWindow *window);

static NSEvent *handleGestureEvent(NSEvent *event, NSWindow *window, void *context,
                                  SFGestureCallback callback, SFScrollCallback scroll) {
    if (!window || sf_gesture_host(event.window) != window) return event;
    if (event.type == NSEventTypeScrollWheel) {
        if (event.phase & NSEventPhaseCancelled) {
            callback(context, 3, 0);
        } else {
            int phase = event.phase & (NSEventPhaseBegan | NSEventPhaseMayBegin) ? 0 :
                        event.phase & NSEventPhaseEnded ? 2 : 1;
            NSEventModifierFlags modifiers = event.modifierFlags;
            unsigned flags = (event.hasPreciseScrollingDeltas ? 1 : 0) |
                (modifiers & NSEventModifierFlagCommand ? 2 : 0) |
                (modifiers & NSEventModifierFlagControl ? 4 : 0) |
                (modifiers & NSEventModifierFlagShift ? 8 : 0) |
                (modifiers & NSEventModifierFlagOption ? 16 : 0);
            // The pointer can be outside the resized host while its swipe is still active.
            scroll(context, event.scrollingDeltaX, event.scrollingDeltaY, phase, flags);
        }
        return nil;
    }
    if (event.type == NSEventTypeMagnify) callback(context, 0, event.magnification);
    else if (event.type == NSEventTypeSmartMagnify) callback(context, 1, 0);
    else if (event.type == NSEventTypeSwipe) callback(context, 2, event.deltaX);
    return nil;
}

@interface SFGestureMonitor : NSObject
@property id events;
@property NSArray *observers;
@end
@implementation SFGestureMonitor
@end

void *sf_install_gestures(void *view_ptr, void *context, SFGestureCallback callback, SFScrollCallback scroll) {
    NSView *view = (__bridge NSView *)view_ptr;
    __weak NSWindow *window = view.window;
    SFGestureMonitor *monitor = [SFGestureMonitor new];
    monitor.events = [NSEvent addLocalMonitorForEventsMatchingMask:
        NSEventMaskMagnify | NSEventMaskSmartMagnify | NSEventMaskSwipe | NSEventMaskScrollWheel
        handler:^NSEvent *(NSEvent *event) {
            return handleGestureEvent(event, window, context, callback, scroll);
        }];
    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    monitor.observers = @[
        [center addObserverForName:@"SFTransitionFinished" object:window queue:nil
                       usingBlock:^(NSNotification *note) { callback(context, 4, [note.userInfo[@"epoch"] doubleValue]); }],
        [center addObserverForName:@"SFTransitionClicked" object:window queue:nil
                       usingBlock:^(__unused NSNotification *note) { callback(context, 3, 0); }],
        [center addObserverForName:NSWindowDidResignKeyNotification object:window queue:nil
                       usingBlock:^(__unused NSNotification *note) { callback(context, 3, 0); }],
        [center addObserverForName:NSApplicationWillResignActiveNotification object:nil queue:nil
                       usingBlock:^(__unused NSNotification *note) { callback(context, 3, 0); }]
    ];
    return (__bridge_retained void *)monitor;
}
void sf_remove_gestures(void *handle) {
    SFGestureMonitor *monitor = CFBridgingRelease(handle);
    [NSEvent removeMonitor:monitor.events];
    for (id observer in monitor.observers) [NSNotificationCenter.defaultCenter removeObserver:observer];
}
void sf_set_icon(const char *path) {
    if ([NSBundle.mainBundle objectForInfoDictionaryKey:@"CFBundleIconFile"]) return;
    NSApp.applicationIconImage = [[NSImage alloc] initWithContentsOfFile:@(path)];
}
