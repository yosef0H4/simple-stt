#!/usr/bin/python3
"""GTK target controlled through private files; run only under the test X server."""
import pathlib
import sys
import gi

gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, GLib

root = pathlib.Path(sys.argv[1])
text = 'مرحبا بالعالم Hello world'
window = Gtk.Window(title='Simple STT private selection target')
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
window.add(box)
entry = Gtk.Entry()
entry.set_text(text)
box.pack_start(entry, False, False, 0)
password = Gtk.Entry()
password.set_visibility(False)
password.set_text(text)
box.pack_start(password, False, False, 0)
button = Gtk.Button(label='No text selection')
box.pack_start(button, False, False, 0)
window.connect('destroy', Gtk.main_quit)
window.show_all()
window.present()
last = None

def poll():
    global last
    try:
        command = (root / 'command').read_text().strip()
    except FileNotFoundError:
        return True
    if command != last:
        last = command
        mode, sequence = command.split(':', 1)
        if mode == 'selected':
            entry.grab_focus()
            entry.select_region(0, -1)
        elif mode == 'empty':
            entry.grab_focus()
            entry.select_region(0, 0)
        elif mode == 'password':
            password.grab_focus()
            password.select_region(0, -1)
        elif mode == 'button':
            button.grab_focus()
        else:
            raise ValueError(mode)
        (root / 'ready').write_text(sequence)
    return True

GLib.timeout_add(10, poll)
Gtk.main()
