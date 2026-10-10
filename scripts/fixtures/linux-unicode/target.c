#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const guint8 binary_payload[] = {0, 1, 'S','i','m','p','l','e','-','S','T','T',' ', 'c','l','i','p','b','o','a','r','d',' ', 'f','i','x','t','u','r','e', 255};
static const guint8 png_payload[] = {
    0x89,0x50,0x4e,0x47,0x0d,0x0a,0x1a,0x0a,0x00,0x00,0x00,0x0d,
    0x49,0x48,0x44,0x52,0x00,0x00,0x00,0x01,0x00,0x00,0x00,0x01,
    0x08,0x06,0x00,0x00,0x00,0x1f,0x15,0xc4,0x89,0x00,0x00,0x00,0x0b,
    0x49,0x44,0x41,0x54,0x78,0x9c,0x63,0x60,0x00,0x02,0x00,0x00,0x05,
    0x00,0x01,0xa5,0xf6,0x45,0x40,0x00,0x00,0x00,0x00,0x49,0x45,0x4e,
    0x44,0xae,0x42,0x60,0x82
};
static const char *result_path;
static void get_data(GtkClipboard *clipboard, GtkSelectionData *selection, guint info, gpointer user_data) {
    (void)clipboard; (void)user_data;
    if (info == 1) gtk_selection_data_set_text(selection, "original clipboard Ω", -1);
    else if (info == 2) gtk_selection_data_set(selection, gtk_selection_data_get_target(selection), 8,
                                                binary_payload, sizeof(binary_payload));
    else if (info == 3) gtk_selection_data_set(selection, gtk_selection_data_get_target(selection), 8,
                                                png_payload, sizeof(png_payload));
}
static void clear_data(GtkClipboard *clipboard, gpointer user_data) { (void)clipboard; (void)user_data; }
static void changed(GtkTextBuffer *buffer, gpointer user_data) {
    (void)user_data;
    GtkTextIter start, end;
    gtk_text_buffer_get_bounds(buffer, &start, &end);
    gchar *text = gtk_text_buffer_get_text(buffer, &start, &end, FALSE);
    FILE *f = fopen(result_path, "wb");
    if (f) { fwrite(text, 1, strlen(text), f); fclose(f); }
    g_free(text);
}
static GtkWidget *target_window, *target_view;
static gchar *focus_path;
static gboolean focus_requested(gpointer data) {
    (void)data;
    if (g_file_test(focus_path, G_FILE_TEST_EXISTS)) {
        gtk_window_present(GTK_WINDOW(target_window));
        gtk_widget_grab_focus(target_view);
        GtkTextBuffer *buffer = gtk_text_view_get_buffer(GTK_TEXT_VIEW(target_view));
        GtkTextIter end;
        gtk_text_buffer_get_end_iter(buffer, &end);
        gtk_text_buffer_place_cursor(buffer, &end);
        remove(focus_path);
    }
    return G_SOURCE_CONTINUE;
}
int main(int argc, char **argv) {
    if (argc != 3) return 2;
    const char *ready_path = argv[1];
    result_path = argv[2];
    gtk_init(&argc, &argv);
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "Simple STT Unicode E2E");
    gtk_window_set_default_size(GTK_WINDOW(window), 640, 160);
    GtkWidget *view = gtk_text_view_new();
    target_window = window;
    target_view = view;
    focus_path = g_strconcat(ready_path, ".focus", NULL);
    g_timeout_add(20, focus_requested, NULL);
    GtkTextBuffer *buffer = gtk_text_view_get_buffer(GTK_TEXT_VIEW(view));
    gtk_text_buffer_set_text(buffer, "before\n", -1);
    GtkWidget *scroll = gtk_scrolled_window_new(NULL, NULL);
    gtk_container_add(GTK_CONTAINER(scroll), view);
    gtk_container_add(GTK_CONTAINER(window), scroll);
    g_signal_connect(buffer, "changed", G_CALLBACK(changed), NULL);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    GtkTargetEntry targets[] = {
        {(gchar *)"text/plain;charset=utf-8", 0, 1},
        {(gchar *)"application/x-simple-stt-e2e", 0, 2},
        {(gchar *)"image/png", 0, 3},
    };
    GtkClipboard *clipboard = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
    if (!gtk_clipboard_set_with_data(clipboard, targets, G_N_ELEMENTS(targets), get_data, clear_data, NULL)) return 3;
    /* Private desktop: keep the editor under the harness focus coordinate. */
    gtk_window_fullscreen(GTK_WINDOW(window));
    gtk_widget_show_all(window);
    gtk_window_present(GTK_WINDOW(window));
    gtk_widget_grab_focus(view);
    FILE *ready = fopen(ready_path, "w");
    if (ready) { fputs("ready", ready); fclose(ready); }
    FILE *result = fopen(result_path, "wb");
    if (result) fclose(result);
    changed(buffer, NULL);
    gtk_main();
    return 0;
}
