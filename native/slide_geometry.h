#pragma once
#include <math.h>

typedef struct { double x, y, width, height; } SFSlideRect;
typedef struct { SFSlideRect outgoing, incoming; } SFSlideFrames;

static inline SFSlideFrames sf_slide_frames(SFSlideRect old, SFSlideRect next, double progress, int direction) {
    double p = fmin(fmax(progress, 0), 1);
    double outScale = 1 - .06 * p, inScale = .94 + .06 * p;
    double outWidth = old.width * outScale, inWidth = next.width * inScale;
    double outHeight = old.height * outScale, inHeight = next.height * inScale;
    double center = (old.x + old.width / 2) * (1 - p) + (next.x + next.width / 2) * p;
    double separation = (outWidth + inWidth) / 2 + 20;
    double sign = direction < 0 ? -1 : 1;
    return (SFSlideFrames){
        {center - sign * p * separation - outWidth / 2,
         old.y + (old.height - outHeight) / 2, outWidth, outHeight},
        {center + sign * (1 - p) * separation - inWidth / 2,
         next.y + (next.height - inHeight) / 2, inWidth, inHeight}
    };
}
