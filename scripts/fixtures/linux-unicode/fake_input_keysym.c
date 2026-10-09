#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wayland-client.h>
#include "fake_input-client-protocol.h"

static struct wl_display *display;
static struct org_kde_kwin_fake_input *fake_input;
static int found;

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version) {
    (void)data;
    if (strcmp(interface, "org_kde_kwin_fake_input") == 0 && version >= 6) {
        fake_input = wl_registry_bind(registry, name,
            &org_kde_kwin_fake_input_interface, 6);
        found = 1;
    }
}
static void registry_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener listener = { registry_global, registry_remove };

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--probe") == 0) {
        display = wl_display_connect(NULL);
        if (!display) return 3;
        struct wl_registry *registry = wl_display_get_registry(display);
        wl_registry_add_listener(registry, &listener, NULL);
        int rc = wl_display_roundtrip(display);
        wl_registry_destroy(registry);
        wl_display_disconnect(display);
        return rc < 0 ? 3 : (found ? 0 : 4);
    }
    int click = argc == 4 && strcmp(argv[1], "--click") == 0;
    int stream = argc == 2 && strcmp(argv[1], "--stream") == 0;
    int keycode_stream = argc == 2 && strcmp(argv[1], "--stream-keycode") == 0;
    if ((!click && !stream && !keycode_stream && argc != 3) || (click && argc != 4)) return 2;
    char *end = NULL;
    unsigned long sym = 0, state = 0;
    if (!click && !stream && !keycode_stream) {
        sym = strtoul(argv[1], &end, 0);
        if (!end || *end || sym > UINT32_MAX) return 2;
        state = strtoul(argv[2], &end, 0);
        if (!end || *end || state > 1) return 2;
    }

    display = wl_display_connect(NULL);
    if (!display) { perror("wl_display_connect"); return 3; }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !found) {
        fprintf(stderr, "KWin fake-input protocol v6 unavailable\n");
        wl_registry_destroy(registry); wl_display_disconnect(display); return 4;
    }
    org_kde_kwin_fake_input_authenticate(fake_input, "Simple STT E2E", "Private Unicode input test");
    if (stream || keycode_stream) {
        if (wl_display_roundtrip(display) < 0) return 5;
        char line[128];
        while (fgets(line, sizeof(line), stdin)) {
            if (sscanf(line, "%lu %lu", &sym, &state) != 2 || sym > UINT32_MAX || state > 1) {
                fprintf(stderr, "invalid stream event\n");
                return 2;
            }
            if (keycode_stream)
                org_kde_kwin_fake_input_keyboard_key(fake_input, (uint32_t)sym, (uint32_t)state);
            else
                org_kde_kwin_fake_input_keyboard_keysym(fake_input, (uint32_t)sym, (uint32_t)state);
            if (wl_display_roundtrip(display) < 0) return 5;
            puts("OK");
            fflush(stdout);
        }
        org_kde_kwin_fake_input_destroy(fake_input);
        wl_registry_destroy(registry);
        wl_display_disconnect(display);
        return 0;
    }
    if (click) {
        org_kde_kwin_fake_input_pointer_motion_absolute(fake_input,
            wl_fixed_from_int(450), wl_fixed_from_int(300));
        org_kde_kwin_fake_input_button(fake_input, 0x110, 1);
        org_kde_kwin_fake_input_button(fake_input, 0x110, 0);
    } else {
        org_kde_kwin_fake_input_keyboard_keysym(fake_input, (uint32_t)sym, (uint32_t)state);
    }
    if (wl_display_roundtrip(display) < 0) {
        perror("fake-input roundtrip"); return 5;
    }
    org_kde_kwin_fake_input_destroy(fake_input);
    wl_registry_destroy(registry);
    wl_display_disconnect(display);
    return 0;
}
