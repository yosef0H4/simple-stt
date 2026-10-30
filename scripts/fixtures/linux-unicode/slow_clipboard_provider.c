#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define IMAGE_FORMATS 37
#define SLOW_FORMATS 3
#define CUSTOM_INDEX (IMAGE_FORMATS + SLOW_FORMATS)
#define PNG_INDEX (CUSTOM_INDEX + 1)
#define TEXT_INDEX (PNG_INDEX + 1)
#define DATA_FORMATS (TEXT_INDEX + 1)
#define TOTAL_FORMATS DATA_FORMATS

static GtkTargetEntry targets[TOTAL_FORMATS];
static gchar names[TOTAL_FORMATS][64];
static guint8 payloads[TOTAL_FORMATS][96];
static gboolean stall_requests;
static gchar *request_log;
static const char *ready_path;
static GtkWidget *provider_window;
static const guint8 custom_payload[] = {0, 1, 'S','i','m','p','l','e','-','S','T','T',' ', 'c','l','i','p','b','o','a','r','d',' ', 'f','i','x','t','u','r','e', 255};
static const guint8 png_payload[] = {
    0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a,0x00,0x00,0x00,0x0d,
    0x49,0x48,0x44,0x52,0x00,0x00,0x00,0x01,0x00,0x00,0x00,0x01,
    0x08,0x06,0x00,0x00,0x00,0x1f,0x15,0xc4,0x89,0x00,0x00,0x00,0x0b,
    0x49,0x44,0x41,0x54,0x78,0x9c,0x63,0x60,0x00,0x02,0x00,0x00,0x05,
    0x00,0x01,0xa5,0xf6,0x45,0x40,0x00,0x00,0x00,0x00,0x49,0x45,0x4e,
    0x44,0xae,0x42,0x60,0x82
};

static void provide_data(GtkClipboard *clipboard, GtkSelectionData *selection,
                         guint info, gpointer user_data) {
    (void)clipboard;
    (void)user_data;
    if (info >= TOTAL_FORMATS) return;
    FILE *log = fopen(request_log, "a");
    if (log) {
        fprintf(log, "%s\n", names[info]);
        fclose(log);
    }
    if (stall_requests) sleep(30);
    if (info >= IMAGE_FORMATS && info < CUSTOM_INDEX) sleep(2);
    if (info == TEXT_INDEX) {
        gtk_selection_data_set_text(selection, "original clipboard Ω", -1);
        return;
    }
    if (info == CUSTOM_INDEX) {
        gtk_selection_data_set(selection, gtk_selection_data_get_target(selection), 8,
                               custom_payload, sizeof(custom_payload));
        return;
    }
    if (info == PNG_INDEX) {
        gtk_selection_data_set(selection, gtk_selection_data_get_target(selection), 8,
                               png_payload, sizeof(png_payload));
        return;
    }
    gtk_selection_data_set(selection, gtk_selection_data_get_target(selection), 8,
                           payloads[info], sizeof(payloads[info]));
}

static void clear_data(GtkClipboard *clipboard, gpointer user_data) {
    (void)clipboard;
    (void)user_data;
}

static void publish(GtkButton *button, gpointer user_data) {
    (void)button;
    (void)user_data;
    GtkClipboard *clipboard = gtk_clipboard_get(g_getenv("SIMPLE_STT_TEST_PRIMARY") ? GDK_SELECTION_PRIMARY : GDK_SELECTION_CLIPBOARD);
    if (!gtk_clipboard_set_with_data(clipboard, targets, TOTAL_FORMATS,
                                     provide_data, clear_data, NULL)) exit(3);
    FILE *ready = fopen(ready_path, "w");
    if (!ready) exit(4);
    fputs("ready", ready);
    fclose(ready);
    gtk_widget_hide(provider_window);
}

static gboolean window_ready(gpointer user_data) {
    (void)user_data;
    gchar *path = g_strconcat(ready_path, ".window", NULL);
    FILE *ready = fopen(path, "w");
    if (!ready) exit(4);
    fputs("mapped", ready);
    fclose(ready);
    g_free(path);
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv) {
    if (argc < 2 || argc > 3) return 2;
    stall_requests = argc == 3 && strcmp(argv[2], "stall") == 0;
    guint seed = argc == 3 && strcmp(argv[2], "alternate") == 0 ? 11 : 0;
    for (guint i = 0; i < IMAGE_FORMATS; ++i) {
        g_snprintf(names[i], sizeof(names[i]), "image/x-simple-stt-e2e-%02u", i);
        targets[i] = (GtkTargetEntry){names[i], 0, i};
    }
    for (guint i = 0; i < SLOW_FORMATS; ++i) {
        g_snprintf(names[IMAGE_FORMATS + i], sizeof(names[0]),
                   "application/postscript;profile=eps-alias-%u", i);
        targets[IMAGE_FORMATS + i] = (GtkTargetEntry){names[IMAGE_FORMATS + i], 0,
                                                        IMAGE_FORMATS + i};
    }
    g_strlcpy(names[CUSTOM_INDEX], "application/x-simple-stt-e2e", sizeof(names[0]));
    targets[CUSTOM_INDEX] = (GtkTargetEntry){names[CUSTOM_INDEX], 0, CUSTOM_INDEX};
    g_strlcpy(names[PNG_INDEX], "image/png", sizeof(names[0]));
    targets[PNG_INDEX] = (GtkTargetEntry){names[PNG_INDEX], 0, PNG_INDEX};
    g_strlcpy(names[TEXT_INDEX], "text/plain;charset=utf-8", sizeof(names[0]));
    targets[TEXT_INDEX] = (GtkTargetEntry){names[TEXT_INDEX], 0, TEXT_INDEX};
    for (guint i = 0; i < TOTAL_FORMATS; ++i) {
        for (guint j = 0; j < sizeof(payloads[i]); ++j)
            payloads[i][j] = (guint8)((i * 29 + j * 17 + 3 + seed) & 0xff);
    }
    gtk_init(&argc, &argv);
    ready_path = argv[1];
    request_log = g_strconcat(argv[1], ".requests", NULL);
    // Clipboard publication on Wayland needs an input serial. This window
    // exists only inside the private test compositor and hides after copying.
    provider_window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(provider_window), "Simple STT Clipboard Fixture");
    gtk_window_fullscreen(GTK_WINDOW(provider_window));
    GtkWidget *button = gtk_button_new_with_label("Copy test clipboard");
    gtk_container_add(GTK_CONTAINER(provider_window), button);
    g_signal_connect(button, "clicked", G_CALLBACK(publish), NULL);
    gtk_widget_show_all(provider_window);
    g_timeout_add(200, window_ready, NULL);
    gtk_main();
    return 0;
}
