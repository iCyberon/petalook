#define SF_UPDATER_TESTING 1
#import "../native/updater.m"
#include <assert.h>

int main(int argc, const char **argv) {
    @autoreleasepool {
        assert(argc == 2);
        assert(validFeed(@"https://github.com/owner/repo/releases/latest/download/appcast.xml"));
        assert(!validFeed(@"http://example.com/update.xml"));
        assert(!validFeed(@"file:///tmp/update.xml"));
        assert(!validFeed(@"https://user:password@example.com/update.xml"));
        NSString *directory = @(argv[1]);
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        sf_updater_start();
        [NSTimer scheduledTimerWithTimeInterval:0.1 repeats:YES block:^(__unused NSTimer *timer) {
            if (delegate.pendingVersion)
                [delegate.pendingVersion writeToFile:[directory stringByAppendingPathComponent:@"ready"]
                    atomically:YES encoding:NSUTF8StringEncoding error:nil];
            if (delegate.lastError)
                [delegate.lastError.description writeToFile:[directory stringByAppendingPathComponent:@"error"]
                    atomically:YES encoding:NSUTF8StringEncoding error:nil];
            if ([NSFileManager.defaultManager fileExistsAtPath:[directory stringByAppendingPathComponent:@"quit"]])
                [NSApp terminate:nil];
        }];
        [NSApp run];
    }
    return 0;
}
