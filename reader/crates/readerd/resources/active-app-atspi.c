#include <atspi/atspi.h>
#include <stdio.h>

int main(void) {
    atspi_init();
    GError *error = NULL;
    AtspiAccessible *desktop = atspi_get_desktop(0);
    if (desktop == NULL) return 2;
    int apps = atspi_accessible_get_child_count(desktop, &error);
    for (int i = 0; i < apps && error == NULL; i++) {
        AtspiAccessible *app = atspi_accessible_get_child_at_index(desktop, i, &error);
        if (app == NULL || error != NULL) break;
        int windows = atspi_accessible_get_child_count(app, &error);
        for (int j = 0; j < windows && error == NULL; j++) {
            AtspiAccessible *window = atspi_accessible_get_child_at_index(app, j, &error);
            if (window == NULL || error != NULL) break;
            AtspiStateSet *states = atspi_accessible_get_state_set(window);
            if (states != NULL && atspi_state_set_contains(states, ATSPI_STATE_ACTIVE)) {
                gchar *name = atspi_accessible_get_name(app, &error);
                if (name != NULL && error == NULL) {
                    puts(name);
                    g_free(name);
                    g_object_unref(states);
                    g_object_unref(window);
                    g_object_unref(app);
                    g_object_unref(desktop);
                    atspi_exit();
                    return 0;
                }
                if (name != NULL) g_free(name);
            }
            if (states != NULL) g_object_unref(states);
            g_object_unref(window);
        }
        g_object_unref(app);
    }
    if (error != NULL) g_error_free(error);
    g_object_unref(desktop);
    atspi_exit();
    return 1;
}
