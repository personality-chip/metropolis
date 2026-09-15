"""Render a captured Windows ConPTY frame, without generating fictional UI art."""
import html
import base64
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path
import pyte

ANSI = {'black': '#101018', 'red': '#e05566', 'green': '#8cdd85', 'brown': '#eac070',
        'blue': '#6b9df4', 'magenta': '#cd8cf4', 'cyan': '#69d6dc', 'white': '#e5e5eb',
        'brightblack': '#5b5e70', 'brightred': '#ff7b8d', 'brightgreen': '#a4f5a2',
        'brightbrown': '#ffe39c', 'brightblue': '#93b9ff', 'brightmagenta': '#e7b4ff',
        'brightcyan': '#a0f4fa', 'brightwhite': '#ffffff'}
def color(value, default):
    if value == 'default':
        return default
    if value in ANSI:
        return ANSI[value]
    return '#' + value if len(value) == 6 else default

def render_svg(capture, path, columns, rows):
    screen = pyte.Screen(columns, rows)
    pyte.Stream(screen).feed(capture)
    w, h = 9, 18
    canvas = Image.new('RGB', (columns*w, rows*h), '#101018')
    draw = ImageDraw.Draw(canvas)
    font = ImageFont.truetype('C:/Windows/Fonts/consola.ttf', 15)
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{columns*w}" height="{rows*h}" viewBox="0 0 {columns*w} {rows*h}">',
             '<rect width="100%" height="100%" fill="#101018"/>',
             '<g font-family="Cascadia Mono,Consolas,DejaVu Sans Mono,monospace" font-size="15">']
    for y in range(rows):
        for x in range(columns):
            cell = screen.buffer[y][x]
            fg, bg = color(cell.fg, '#e5e5eb'), color(cell.bg, '#101018')
            if cell.reverse:
                fg, bg = bg, fg
            draw.rectangle((x*w,y*h,(x+1)*w-1,(y+1)*h-1), fill=bg)
            if bg != '#101018':
                parts.append(f'<rect x="{x*w}" y="{y*h}" width="{w}" height="{h}" fill="{bg}"/>')
            if cell.data.strip():
                draw.text((x*w,y*h-1), cell.data, font=font, fill=fg)
                parts.append(f'<text x="{x*w}" y="{y*h+14}" fill="{fg}">{html.escape(cell.data)}</text>')
    parts.append('</g></svg>')
    Path(path).write_text(''.join(parts), encoding='utf-8')
    png = Path(path).with_suffix('.png')
    canvas.save(png)
    print('TERMINAL_PNG_BASE64=' + base64.b64encode(png.read_bytes()).decode())
