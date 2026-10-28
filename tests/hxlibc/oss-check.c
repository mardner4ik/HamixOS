#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <linux/soundcard.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

int main(void) {
    int dsp = open("/dev/dsp", O_WRONLY | O_CLOEXEC);
    check(dsp >= 0, "open /dev/dsp");
    if (dsp < 0) {
        printf("oss-check: errno %d\n", errno);
        return 1;
    }
    struct stat st;
    check(fstat(dsp, &st) == 0 && S_ISCHR(st.st_mode) && major(st.st_rdev) == 14 && minor(st.st_rdev) == 3, "/dev/dsp is char 14:3");
    int fmt = AFMT_S16_LE, channels = 2, rate = 44100;
    check(ioctl(dsp, SNDCTL_DSP_SETFMT, &fmt) == 0 && fmt == AFMT_S16_LE, "SNDCTL_DSP_SETFMT S16_LE");
    check(ioctl(dsp, SNDCTL_DSP_CHANNELS, &channels) == 0 && channels == 2, "SNDCTL_DSP_CHANNELS 2");
    check(ioctl(dsp, SNDCTL_DSP_SPEED, &rate) == 0 && rate == 44100, "SNDCTL_DSP_SPEED 44100");
    int frag = (8 << 16) | 12;
    check(ioctl(dsp, SNDCTL_DSP_SETFRAGMENT, &frag) == 0, "SNDCTL_DSP_SETFRAGMENT");
    static short samples[44100 / 2 * 2];
    for (int i = 0; i < 44100 / 2; i++) {
        short v = (short)(12000 * sin(2 * M_PI * 440.0 * i / 44100.0));
        samples[2 * i] = v;
        samples[2 * i + 1] = v;
    }
    size_t done = 0;
    while (done < sizeof samples) {
        ssize_t n = write(dsp, (char *)samples + done, sizeof samples - done);
        if (n <= 0)
            break;
        done += (size_t)n;
    }
    check(done == sizeof samples, "write half a second of 440 Hz");
    audio_buf_info info;
    check(ioctl(dsp, SNDCTL_DSP_GETOSPACE, &info) == 0 && info.fragsize > 0, "SNDCTL_DSP_GETOSPACE");
    int delay = -1;
    check(ioctl(dsp, SNDCTL_DSP_GETODELAY, &delay) == 0 && delay >= 0, "SNDCTL_DSP_GETODELAY");
    check(ioctl(dsp, SNDCTL_DSP_SYNC, 0) == 0, "SNDCTL_DSP_SYNC drains");
    check(ioctl(dsp, SNDCTL_DSP_GETODELAY, &delay) == 0 && delay == 0, "nothing queued after SYNC");
    close(dsp);
    int mixer = open("/dev/mixer", O_RDWR);
    check(mixer >= 0, "open /dev/mixer");
    int devmask = 0, volume = 0;
    check(ioctl(mixer, SOUND_MIXER_READ_DEVMASK, &devmask) == 0 && (devmask & SOUND_MASK_VOLUME), "SOUND_MIXER_READ_DEVMASK has VOLUME");
    int set = 40 | (40 << 8);
    check(ioctl(mixer, SOUND_MIXER_WRITE_VOLUME, &set) == 0, "SOUND_MIXER_WRITE_VOLUME");
    check(ioctl(mixer, SOUND_MIXER_READ_VOLUME, &volume) == 0 && (volume & 0xff) == 40, "SOUND_MIXER_READ_VOLUME reads it back");
    close(mixer);
    return failures ? 1 : 0;
}
