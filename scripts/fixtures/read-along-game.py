"""OpenGL-rendered dialogue fixture; no real game or game assets required."""
import array
import ctypes
import html
import os
import re
os.environ.setdefault('PYOPENGL_PLATFORM', 'egl')
import cairo
import gi
gi.require_version('Gtk', '3.0')
gi.require_version('PangoCairo', '1.0')
from gi.repository import Gtk, Pango, PangoCairo
from OpenGL import GL


class GameDialogue(Gtk.GLArea):
    def __init__(self):
        super().__init__()
        self.set_required_version(3, 2)
        self.text = ''
        self.program = None
        self.connect('render', self.render)

    def set_markup(self, text):
        self.set_text(html.unescape(re.sub('<[^>]+>', '', text)))

    def set_text(self, text):
        self.text = text
        self.queue_render()

    def initialize(self):
        def shader(kind, source):
            item = GL.glCreateShader(kind)
            GL.glShaderSource(item, source)
            GL.glCompileShader(item)
            assert GL.glGetShaderiv(item, GL.GL_COMPILE_STATUS), GL.glGetShaderInfoLog(item)
            return item
        vertex = shader(GL.GL_VERTEX_SHADER, '#version 150\nin vec2 position; in vec2 texcoord; out vec2 uv; void main(){uv=texcoord;gl_Position=vec4(position,0,1);}')
        fragment = shader(GL.GL_FRAGMENT_SHADER, '#version 150\nuniform sampler2D image; in vec2 uv; out vec4 color; void main(){color=texture(image,uv);}')
        self.program = GL.glCreateProgram()
        GL.glAttachShader(self.program, vertex)
        GL.glAttachShader(self.program, fragment)
        GL.glLinkProgram(self.program)
        assert GL.glGetProgramiv(self.program, GL.GL_LINK_STATUS), GL.glGetProgramInfoLog(self.program)
        GL.glDeleteShader(vertex)
        GL.glDeleteShader(fragment)
        self.vao = GL.glGenVertexArrays(1)
        GL.glBindVertexArray(self.vao)
        self.vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.vbo)
        vertices = array.array('f', [-1,-1,0,1, 1,-1,1,1, -1,1,0,0, -1,1,0,0, 1,-1,1,1, 1,1,1,0]).tobytes()
        GL.glBufferData(GL.GL_ARRAY_BUFFER, len(vertices), vertices, GL.GL_STATIC_DRAW)
        for name, offset in [('position', 0), ('texcoord', 8)]:
            location = GL.glGetAttribLocation(self.program, name)
            GL.glEnableVertexAttribArray(location)
            GL.glVertexAttribPointer(location, 2, GL.GL_FLOAT, False, 16, ctypes.c_void_p(offset))
        self.texture = GL.glGenTextures(1)
        print('Game fixture OpenGL renderer:', GL.glGetString(GL.GL_RENDERER).decode(), flush=True)

    def render(self, _area, _context):
        if self.program is None:
            self.initialize()
        width, height = self.get_allocated_width(), self.get_allocated_height()
        image = cairo.ImageSurface(cairo.FORMAT_ARGB32, width, height)
        painter = cairo.Context(image)
        painter.set_source_rgb(.08, .12, .2)
        painter.paint()
        painter.set_source_rgb(.38, .22, .12)
        painter.rectangle(30, 40, width-60, max(20, height-260))
        painter.fill()
        painter.set_source_rgb(.02, .03, .06)
        painter.rectangle(30, height-205, width-60, 185)
        painter.fill_preserve()
        painter.set_source_rgb(.4, .65, .95)
        painter.set_line_width(3)
        painter.stroke()
        painter.set_source_rgb(1, 1, 1)
        painter.move_to(50, height-192)
        layout = PangoCairo.create_layout(painter)
        layout.set_font_description(Pango.FontDescription('DejaVu Sans 20'))
        layout.set_width((width-100)*Pango.SCALE)
        layout.set_wrap(Pango.WrapMode.WORD_CHAR)
        layout.set_text('Counsel\n' + self.text if self.text else '', -1)
        PangoCairo.show_layout(painter, layout)
        image.flush()
        GL.glUseProgram(self.program)
        GL.glBindVertexArray(self.vao)
        GL.glActiveTexture(GL.GL_TEXTURE0)
        GL.glBindTexture(GL.GL_TEXTURE_2D, self.texture)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MIN_FILTER, GL.GL_LINEAR)
        GL.glTexParameteri(GL.GL_TEXTURE_2D, GL.GL_TEXTURE_MAG_FILTER, GL.GL_LINEAR)
        GL.glTexImage2D(GL.GL_TEXTURE_2D, 0, GL.GL_RGBA8, width, height, 0, GL.GL_BGRA, GL.GL_UNSIGNED_BYTE, image.get_data())
        GL.glUniform1i(GL.glGetUniformLocation(self.program, 'image'), 0)
        scale = self.get_scale_factor()
        GL.glViewport(0, 0, width*scale, height*scale)
        GL.glDrawArrays(GL.GL_TRIANGLES, 0, 6)
        return True
