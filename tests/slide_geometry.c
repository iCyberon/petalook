#include "../native/slide_geometry.h"
#include <assert.h>
#include <stdio.h>

static void close_to(double actual, double expected) { assert(fabs(actual - expected) < .000001); }
int main(void) {
    SFSlideRect frames[] = {{150, 120, 800, 600}, {700, 40, 300, 900}, {-1200, 60, 1100, 240}};
    for (int a = 0; a < 3; a++) for (int b = 0; b < 3; b++) for (int direction = -1; direction <= 1; direction += 2) {
        SFSlideFrames start = sf_slide_frames(frames[a], frames[b], 0, direction);
        SFSlideFrames end = sf_slide_frames(frames[a], frames[b], 1, direction);
        close_to(start.outgoing.x, frames[a].x);
        close_to(start.outgoing.y, frames[a].y);
        close_to(start.outgoing.width, frames[a].width);
        close_to(end.incoming.x, frames[b].x);
        close_to(end.incoming.y, frames[b].y);
        close_to(end.incoming.width, frames[b].width);
        for (int step = 0; step <= 1000; step++) {
            SFSlideFrames f = sf_slide_frames(frames[a], frames[b], step / 1000., direction);
            double gap = direction > 0 ? f.incoming.x - f.outgoing.x - f.outgoing.width
                                       : f.outgoing.x - f.incoming.x - f.incoming.width;
            close_to(gap, 20);
            assert(f.outgoing.width > 0 && f.incoming.width > 0);
            close_to(f.outgoing.width / f.outgoing.height, frames[a].width / frames[a].height);
            close_to(f.incoming.width / f.incoming.height, frames[b].width / frames[b].height);
        }
    }
    puts("Slide geometry: endpoints, aspect ratios and non-overlap passed (18,018 frames).");
}
