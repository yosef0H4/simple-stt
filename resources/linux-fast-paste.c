/*
 * Linux fast paste helper adapted from OpenWhispr (MIT License).
 * OpenWhispr source: resources/linux-fast-paste.c.
 * Keep OpenWhispr's license notice when redistributing this helper.
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>
#include <unistd.h>

#ifdef HAVE_ATSPI
#include <atspi/atspi.h>
#endif

typedef enum {
    PASTE_MODE_CTRL_V        = 0,
    PASTE_MODE_CTRL_SHIFT_V  = 1,
    PASTE_MODE_SHIFT_INSERT  = 2,
    COPY_MODE_CTRL_INSERT = 3,
    COPY_MODE_CTRL_C = 4,
    COPY_MODE_CTRL_SHIFT_C = 5,
} paste_mode_t;

#ifdef HAVE_GIO
#include <gio/gio.h>

#define PORTAL_BUS   "org.freedesktop.portal.Desktop"
#define PORTAL_PATH  "/org/freedesktop/portal/desktop"
#define PORTAL_IFACE "org.freedesktop.portal.RemoteDesktop"
#define REQUEST_IFACE "org.freedesktop.portal.Request"

/* evdev keycodes for portal mode */
#define PORTAL_KEY_LEFTCTRL  29
#define PORTAL_KEY_LEFTSHIFT 42
#define PORTAL_KEY_V         47
#define PORTAL_KEY_C         46
#define PORTAL_KEY_INSERT    110

static int portal_exit_code = 0;

typedef struct {
    GDBusConnection *conn;
    GMainLoop       *loop;
    char            *session_handle;
    char            *restore_token;
    guint            signal_id;
    paste_mode_t     mode;
} PortalData;

static char *get_sender_path(GDBusConnection *conn)
{
    const char *name = g_dbus_connection_get_unique_name(conn);
    char *path = g_strdup(name + 1);
    for (char *p = path; *p; p++) {
        if (*p == '.') *p = '_';
    }
    return path;
}

static guint subscribe_response(PortalData *app, const char *request_path,
                                GDBusSignalCallback callback)
{
    return g_dbus_connection_signal_subscribe(
        app->conn, PORTAL_BUS, REQUEST_IFACE, "Response",
        request_path, NULL, G_DBUS_SIGNAL_FLAGS_NO_MATCH_RULE,
        callback, app, NULL);
}

static void portal_emit_key(PortalData *app, gint32 keycode, guint32 pressed,
                            const char *label)
{
    GError *err = NULL;
    GVariant *opts = g_variant_new("a{sv}", NULL);
    g_dbus_connection_call_sync(app->conn, PORTAL_BUS, PORTAL_PATH,
        PORTAL_IFACE, "NotifyKeyboardKeycode",
        g_variant_new("(o@a{sv}iu)", app->session_handle, opts, keycode, pressed),
        NULL, G_DBUS_CALL_FLAGS_NONE, -1, NULL, &err);
    if (err) {
        fprintf(stderr, "%s: %s\n", label, err->message);
        portal_exit_code = 1;
        g_clear_error(&err);
    }
}

static void portal_send_paste(PortalData *app)
{
    if (app->mode == PASTE_MODE_SHIFT_INSERT) {
        portal_emit_key(app, PORTAL_KEY_LEFTSHIFT, 1, "Shift press");
        portal_emit_key(app, PORTAL_KEY_INSERT,    1, "Insert press");
        usleep(20000);
        portal_emit_key(app, PORTAL_KEY_INSERT,    0, "Insert release");
        portal_emit_key(app, PORTAL_KEY_LEFTSHIFT, 0, "Shift release");
    } else {
        const int use_shift = (app->mode == PASTE_MODE_CTRL_SHIFT_V || app->mode == COPY_MODE_CTRL_SHIFT_C);
        const int key = app->mode == COPY_MODE_CTRL_INSERT ? PORTAL_KEY_INSERT :
            (app->mode == COPY_MODE_CTRL_C || app->mode == COPY_MODE_CTRL_SHIFT_C ? PORTAL_KEY_C : PORTAL_KEY_V);

        portal_emit_key(app, PORTAL_KEY_LEFTCTRL, 1, "Ctrl press");
        if (use_shift)
            portal_emit_key(app, PORTAL_KEY_LEFTSHIFT, 1, "Shift press");
        portal_emit_key(app, key, 1, "key press");
        usleep(20000);
        portal_emit_key(app, key, 0, "key release");
        if (use_shift)
            portal_emit_key(app, PORTAL_KEY_LEFTSHIFT, 0, "Shift release");
        portal_emit_key(app, PORTAL_KEY_LEFTCTRL, 0, "Ctrl release");
    }

    g_main_loop_quit(app->loop);
}

static void on_start_response(GDBusConnection *conn, const char *sender,
    const char *object_path, const char *interface_name,
    const char *signal_name, GVariant *parameters, gpointer user_data)
{
    PortalData *app = user_data;
    guint32 response;
    GVariant *results;

    g_variant_get(parameters, "(u@a{sv})", &response, &results);
    g_dbus_connection_signal_unsubscribe(app->conn, app->signal_id);

    if (response != 0) {
        fprintf(stderr, "Start cancelled (response=%u)\n", response);
        portal_exit_code = 3;
        g_variant_unref(results);
        g_main_loop_quit(app->loop);
        return;
    }

    guint32 devices = 0;
    g_variant_lookup(results, "devices", "u", &devices);
    fprintf(stderr, "Portal granted devices=%u\n", devices);
    if (!(devices & 1)) {
        fprintf(stderr, "Portal did not grant keyboard control\n");
        portal_exit_code = 3;
        g_variant_unref(results);
        g_main_loop_quit(app->loop);
        return;
    }

    GVariant *token_v = g_variant_lookup_value(results, "restore_token",
                                                G_VARIANT_TYPE_STRING);
    if (token_v) {
        const char *token = g_variant_get_string(token_v, NULL);
        printf("%s\n", token);
        fflush(stdout);
        g_variant_unref(token_v);
    }

    g_variant_unref(results);
    portal_send_paste(app);
}

static void on_select_devices_response(GDBusConnection *conn, const char *sender,
    const char *object_path, const char *interface_name,
    const char *signal_name, GVariant *parameters, gpointer user_data)
{
    PortalData *app = user_data;
    guint32 response;
    GVariant *results;

    g_variant_get(parameters, "(u@a{sv})", &response, &results);
    g_dbus_connection_signal_unsubscribe(app->conn, app->signal_id);
    g_variant_unref(results);

    if (response != 0) {
        fprintf(stderr, "SelectDevices denied (response=%u)\n", response);
        portal_exit_code = 2;
        g_main_loop_quit(app->loop);
        return;
    }

    char *sender_path = get_sender_path(app->conn);
    char *request_path = g_strdup_printf(
        "/org/freedesktop/portal/desktop/request/%s/start", sender_path);
    g_free(sender_path);

    app->signal_id = subscribe_response(app, request_path, on_start_response);

    GVariantBuilder opts;
    g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&opts, "{sv}", "handle_token",
                          g_variant_new_string("start"));

    GError *err = NULL;
    g_dbus_connection_call_sync(app->conn, PORTAL_BUS, PORTAL_PATH,
        PORTAL_IFACE, "Start",
        g_variant_new("(os@a{sv})", app->session_handle, "",
                       g_variant_builder_end(&opts)),
        NULL, G_DBUS_CALL_FLAGS_NONE, -1, NULL, &err);

    g_free(request_path);
    if (err) {
        fprintf(stderr, "Start call failed: %s\n", err->message);
        g_error_free(err);
        portal_exit_code = 1;
        g_main_loop_quit(app->loop);
    }
}

static void on_create_session_response(GDBusConnection *conn, const char *sender,
    const char *object_path, const char *interface_name,
    const char *signal_name, GVariant *parameters, gpointer user_data)
{
    PortalData *app = user_data;
    guint32 response;
    GVariant *results;

    g_variant_get(parameters, "(u@a{sv})", &response, &results);
    g_dbus_connection_signal_unsubscribe(app->conn, app->signal_id);

    if (response != 0) {
        fprintf(stderr, "CreateSession denied (response=%u)\n", response);
        portal_exit_code = 2;
        g_variant_unref(results);
        g_main_loop_quit(app->loop);
        return;
    }

    GVariant *handle_v = g_variant_lookup_value(results, "session_handle",
                                                 G_VARIANT_TYPE_STRING);
    app->session_handle = g_variant_dup_string(handle_v, NULL);
    g_variant_unref(handle_v);
    g_variant_unref(results);

    char *sender_path = get_sender_path(app->conn);
    char *request_path = g_strdup_printf(
        "/org/freedesktop/portal/desktop/request/%s/selectdevices",
        sender_path);
    g_free(sender_path);

    app->signal_id = subscribe_response(app, request_path,
                                        on_select_devices_response);

    GVariantBuilder opts;
    g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&opts, "{sv}", "handle_token",
                          g_variant_new_string("selectdevices"));
    g_variant_builder_add(&opts, "{sv}", "types",
                          g_variant_new_uint32(1)); /* KEYBOARD only */
    g_variant_builder_add(&opts, "{sv}", "persist_mode",
                          g_variant_new_uint32(2)); /* persistent */

    if (app->restore_token) {
        g_variant_builder_add(&opts, "{sv}", "restore_token",
                              g_variant_new_string(app->restore_token));
    }

    GError *err = NULL;
    g_dbus_connection_call_sync(app->conn, PORTAL_BUS, PORTAL_PATH,
        PORTAL_IFACE, "SelectDevices",
        g_variant_new("(o@a{sv})", app->session_handle,
                       g_variant_builder_end(&opts)),
        NULL, G_DBUS_CALL_FLAGS_NONE, -1, NULL, &err);

    g_free(request_path);
    if (err) {
        fprintf(stderr, "SelectDevices call failed: %s\n", err->message);
        g_error_free(err);
        portal_exit_code = 1;
        g_main_loop_quit(app->loop);
    }
}

static gboolean on_portal_timeout(gpointer user_data)
{
    PortalData *app = user_data;
    fprintf(stderr, "Timeout waiting for portal response\n");
    portal_exit_code = 1;
    g_main_loop_quit(app->loop);
    return G_SOURCE_REMOVE;
}

static int paste_via_portal(paste_mode_t mode, const char *restore_token)
{
    PortalData app = { 0 };
    app.mode = mode;
    if (restore_token) app.restore_token = g_strdup(restore_token);

    GError *err = NULL;
    app.conn = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &err);
    if (!app.conn) {
        fprintf(stderr, "D-Bus connection failed: %s\n", err->message);
        g_error_free(err);
        g_free(app.restore_token);
        return 1;
    }

    app.loop = g_main_loop_new(NULL, FALSE);
    g_timeout_add_seconds(60, on_portal_timeout, &app);

    char *sender_path = get_sender_path(app.conn);
    char *request_path = g_strdup_printf(
        "/org/freedesktop/portal/desktop/request/%s/createsession",
        sender_path);
    g_free(sender_path);

    app.signal_id = subscribe_response(&app, request_path,
                                       on_create_session_response);

    GVariantBuilder opts;
    g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&opts, "{sv}", "handle_token",
                          g_variant_new_string("createsession"));
    g_variant_builder_add(&opts, "{sv}", "session_handle_token",
                          g_variant_new_string("simple_stt"));

    g_dbus_connection_call_sync(app.conn, PORTAL_BUS, PORTAL_PATH,
        PORTAL_IFACE, "CreateSession",
        g_variant_new("(@a{sv})", g_variant_builder_end(&opts)),
        NULL, G_DBUS_CALL_FLAGS_NONE, -1, NULL, &err);

    g_free(request_path);
    if (err) {
        fprintf(stderr, "CreateSession failed: %s\n", err->message);
        g_error_free(err);
        g_main_loop_unref(app.loop);
        g_free(app.restore_token);
        g_object_unref(app.conn);
        return 1;
    }

    g_main_loop_run(app.loop);

    g_main_loop_unref(app.loop);
    g_free(app.session_handle);
    g_free(app.restore_token);
    g_object_unref(app.conn);

    return portal_exit_code;
}
#endif /* HAVE_GIO */

static const char *terminal_classes[] = {
    "konsole", "gnome-terminal", "terminal", "kitty", "alacritty",
    "terminator", "xterm", "urxvt", "rxvt", "tilix", "terminology",
    "wezterm", "foot", "st", "yakuake", "ghostty", "guake", "tilda",
    "hyper", "tabby", "sakura", "warp", "termius", NULL
};

static int is_terminal(const char *wm_class) {
    if (!wm_class) return 0;
    for (int i = 0; terminal_classes[i]; i++) {
        if (strcasestr(wm_class, terminal_classes[i]))
            return 1;
    }
    return 0;
}

static int check_parent_terminal(Display *dpy, Window win) {
    Window current = win;
    Window root = DefaultRootWindow(dpy);

    for (int depth = 0; depth < 20; depth++) {
        Window parent, dummy_root;
        Window *children = NULL;
        unsigned int nchildren;

        if (!XQueryTree(dpy, current, &dummy_root, &parent, &children, &nchildren)) {
            if (children) XFree(children);
            break;
        }
        if (children) XFree(children);
        if (parent == 0 || parent == root) break;

        XClassHint hint;
        if (XGetClassHint(dpy, parent, &hint)) {
            int terminal = is_terminal(hint.res_class) || is_terminal(hint.res_name);
            XFree(hint.res_name);
            XFree(hint.res_class);
            return terminal;
        }

        current = parent;
    }

    return 0;
}

#ifdef HAVE_ATSPI
static AtspiAccessible *focused_text(AtspiAccessible *node, int depth, int *budget) {
    if (!node || depth > 24 || --*budget < 0) return NULL;
    AtspiStateSet *states = atspi_accessible_get_state_set(node);
    gboolean focused = states && atspi_state_set_contains(states, ATSPI_STATE_FOCUSED);
    if (states) g_object_unref(states);
    if (focused && atspi_accessible_get_role(node, NULL) != ATSPI_ROLE_PASSWORD_TEXT) {
        AtspiText *text = atspi_accessible_get_text_iface(node);
        if (text) { g_object_unref(text); return g_object_ref(node); }
    }
    int count = atspi_accessible_get_child_count(node, NULL);
    for (int i = 0; i < count && *budget > 0; ++i) {
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, i, NULL);
        AtspiAccessible *found = focused_text(child, depth + 1, budget);
        if (child) g_object_unref(child);
        if (found) return found;
    }
    return NULL;
}

/* 0: selection on stdout, 1: unavailable/error, 2: no selected text. */
static int read_selection_direct(void) {
#ifdef HAVE_GIO
    GError *bus_error = NULL;
    GDBusConnection *bus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &bus_error);
    GVariant *address = bus ? g_dbus_connection_call_sync(bus, "org.a11y.Bus", "/org/a11y/bus",
        "org.a11y.Bus", "GetAddress", NULL, G_VARIANT_TYPE("(s)"), G_DBUS_CALL_FLAGS_NONE,
        250, NULL, &bus_error) : NULL;
    if (address) g_variant_unref(address);
    if (bus) g_object_unref(bus);
    if (bus_error) { g_error_free(bus_error); return 1; }
#endif
    atspi_init(); atspi_set_timeout(300, 1000);
    AtspiAccessible *desktop = atspi_get_desktop(0), *target = NULL;
    int budget = 512;
    int apps = desktop ? atspi_accessible_get_child_count(desktop, NULL) : 0;
    for (int i = 0; i < apps && !target; ++i) {
        AtspiAccessible *app = atspi_accessible_get_child_at_index(desktop, i, NULL);
        int windows = app ? atspi_accessible_get_child_count(app, NULL) : 0;
        for (int j = 0; j < windows && !target; ++j) {
            AtspiAccessible *win = atspi_accessible_get_child_at_index(app, j, NULL);
            AtspiStateSet *states = win ? atspi_accessible_get_state_set(win) : NULL;
            gboolean active = states && atspi_state_set_contains(states, ATSPI_STATE_ACTIVE);
            if (states) g_object_unref(states);
            if (active) target = focused_text(win, 0, &budget);
            if (win) g_object_unref(win);
        }
        if (app) g_object_unref(app);
    }
    int result = 1;
    AtspiText *text = target ? atspi_accessible_get_text_iface(target) : NULL;
    GError *error = NULL;
    if (text) {
        int count = atspi_text_get_n_selections(text, &error);
        if (!error && count == 0) result = 2;
        if (!error && count == 1) {
            AtspiRange *range = atspi_text_get_selection(text, 0, &error);
            if (!error && range && range->start_offset >= 0 &&
                range->end_offset >= range->start_offset &&
                range->end_offset - range->start_offset <= 1024 * 1024) {
                if (range->start_offset == range->end_offset) result = 2;
                else {
                    char *selected = atspi_text_get_text(text, range->start_offset, range->end_offset, &error);
                    if (!error && selected && strlen(selected) <= 1024 * 1024 &&
                        g_utf8_validate(selected, -1, NULL)) {
                        size_t len = strlen(selected);
                        result = len == 0 ? 2 : (fwrite(selected, 1, len, stdout) == len ? 0 : 1);
                    }
                    g_free(selected);
                }
            }
            g_free(range);
        }
    }
    if (error) g_error_free(error);
    if (text) g_object_unref(text);
    if (target) g_object_unref(target);
    if (desktop) g_object_unref(desktop);
    return result;
}

/* Refresh AT-SPI's cached states before every paced insertion. A target that
 * loses focus or whose original top-level window loses activation is no longer
 * safe, even if another control has the same application identity. */
static char *active_app_atspi(void) {
    atspi_init();
    AtspiAccessible *desktop = atspi_get_desktop(0);
    if (!desktop) return NULL;

    int n = atspi_accessible_get_child_count(desktop, NULL);
    for (int i = 0; i < n; i++) {
        AtspiAccessible *app = atspi_accessible_get_child_at_index(desktop, i, NULL);
        if (!app) continue;
        int nw = atspi_accessible_get_child_count(app, NULL);
        for (int j = 0; j < nw; j++) {
            AtspiAccessible *win = atspi_accessible_get_child_at_index(app, j, NULL);
            if (!win) continue;
            AtspiStateSet *states = atspi_accessible_get_state_set(win);
            gboolean active = atspi_state_set_contains(states, ATSPI_STATE_ACTIVE);
            g_object_unref(states);
            if (active) {
                char *name = atspi_accessible_get_name(app, NULL);
                g_object_unref(win);
                g_object_unref(app);
                return name;
            }
            g_object_unref(win);
        }
        g_object_unref(app);
    }
    return NULL;
}

static int detect_terminal_atspi(void) {
    char *name = active_app_atspi();
    if (!name) return -1;
    int result = is_terminal(name);
    g_free(name);
    return result;
}
#endif

static Window get_active_window(Display *dpy) {
    Atom prop = XInternAtom(dpy, "_NET_ACTIVE_WINDOW", True);
    if (prop != None) {
        Atom actual_type;
        int actual_format;
        unsigned long nitems, bytes_after;
        unsigned char *data = NULL;

        if (XGetWindowProperty(dpy, DefaultRootWindow(dpy), prop, 0, 1, False,
                               XA_WINDOW, &actual_type, &actual_format,
                               &nitems, &bytes_after, &data) == Success && data) {
            Window win = nitems > 0 ? *(Window *)data : None;
            XFree(data);
            if (win != None) return win;
        }
    }

    Window focused;
    int revert;
    XGetInputFocus(dpy, &focused, &revert);
    return focused;
}


/* Observation and portal shortcuts only. No XTest, uinput, accessibility writes,
 * or window activation. The application uses the Rust EIS broker for input. */
int main(int argc, char *argv[]) {
    int force_terminal = 0, force_ctrl_v = 0, force_shift_insert = 0;
    int use_portal = 0, detect_terminal = 0, print_active_app = 0, read_selection = 0;
    int copy_mode = -1;
    const char *restore_token = NULL;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--read-selection")) read_selection = 1;
        else if (!strcmp(argv[i], "--active-app")) print_active_app = 1;
        else if (!strcmp(argv[i], "--detect-terminal")) detect_terminal = 1;
        else if (!strcmp(argv[i], "--portal")) use_portal = 1;
        else if (!strcmp(argv[i], "--ctrl-v")) force_ctrl_v = 1;
        else if (!strcmp(argv[i], "--terminal")) force_terminal = 1;
        else if (!strcmp(argv[i], "--shift-insert")) force_shift_insert = 1;
        else if (!strcmp(argv[i], "--restore-token") && i + 1 < argc) restore_token = argv[++i];
        else if (!strcmp(argv[i], "--copy-key") && i + 1 < argc) {
            const char *key = argv[++i];
            if (!strcmp(key, "ctrl-insert")) copy_mode = COPY_MODE_CTRL_INSERT;
            else if (!strcmp(key, "ctrl-c")) copy_mode = COPY_MODE_CTRL_C;
            else if (!strcmp(key, "ctrl-shift-c")) copy_mode = COPY_MODE_CTRL_SHIFT_C;
            else return 1;
        } else { fprintf(stderr, "unsupported helper action; input requires the RemoteDesktop portal\n"); return 1; }
    }
    if (read_selection) {
#ifdef HAVE_ATSPI
        return read_selection_direct();
#else
        return 1;
#endif
    }
    if (detect_terminal) {
#ifdef HAVE_ATSPI
        int atspi_result = detect_terminal_atspi();
        if (atspi_result >= 0) return atspi_result ? 0 : 1;
#endif
        Display *dpy = XOpenDisplay(NULL);
        if (!dpy) return 2;
        Window win = get_active_window(dpy);
        int result = 0;
        XClassHint hint;
        if (win != None && XGetClassHint(dpy, win, &hint)) {
            result = is_terminal(hint.res_class) || is_terminal(hint.res_name);
            XFree(hint.res_name);
            XFree(hint.res_class);
        } else if (win != None) {
            result = check_parent_terminal(dpy, win);
        }
        XCloseDisplay(dpy);
        return result ? 0 : 1;
    }

    if (print_active_app) {
#ifdef HAVE_ATSPI
        char *name = active_app_atspi();
        if (name && *name) {
            puts(name);
            g_free(name);
            return 0;
        }
        g_free(name);
#endif
        Display *dpy = XOpenDisplay(NULL);
        if (!dpy) return 2;
        Window win = get_active_window(dpy);
        XClassHint hint;
        if (win != None && XGetClassHint(dpy, win, &hint)) {
            const char *identity = hint.res_class && *hint.res_class ? hint.res_class : hint.res_name;
            int found = identity && *identity;
            if (identity && *identity) puts(identity);
            XFree(hint.res_name);
            XFree(hint.res_class);
            XCloseDisplay(dpy);
            return found ? 0 : 2;
        }
        XCloseDisplay(dpy);
        return 2;
    }


    if (!use_portal) { fprintf(stderr, "input requires the RemoteDesktop portal\n"); return 1; }
#ifdef HAVE_GIO
    paste_mode_t mode = copy_mode >= 0 ? (paste_mode_t)copy_mode :
        force_shift_insert ? PASTE_MODE_SHIFT_INSERT :
        force_ctrl_v ? PASTE_MODE_CTRL_V :
        force_terminal ? PASTE_MODE_CTRL_SHIFT_V : PASTE_MODE_CTRL_V;
    return paste_via_portal(mode, restore_token);
#else
    (void)copy_mode; (void)force_terminal; (void)force_ctrl_v;
    (void)force_shift_insert; (void)restore_token;
    fprintf(stderr, "portal support not compiled in\n"); return 3;
#endif
}
