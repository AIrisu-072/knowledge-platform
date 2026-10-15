# TEST-ONLY: asks an X window to close, as its title-bar close button or a
# window manager's Alt+F4 does: a WM_PROTOCOLS/WM_DELETE_WINDOW client message.
# (xdotool 3.20160805 has no such command.) Usage: x11-close.py <window id>
import ctypes
import sys

x = ctypes.cdll.LoadLibrary('libX11.so.6')
x.XOpenDisplay.restype = ctypes.c_void_p
x.XOpenDisplay.argtypes = [ctypes.c_char_p]
x.XInternAtom.restype = ctypes.c_ulong
x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]


class XClientMessageEvent(ctypes.Structure):
    _fields_ = [('type', ctypes.c_int), ('serial', ctypes.c_ulong), ('send_event', ctypes.c_int),
                ('display', ctypes.c_void_p), ('window', ctypes.c_ulong), ('message_type', ctypes.c_ulong),
                ('format', ctypes.c_int), ('data', ctypes.c_long * 5)]


class XEvent(ctypes.Union):
    _fields_ = [('xclient', XClientMessageEvent), ('pad', ctypes.c_long * 24)]


x.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_long, ctypes.POINTER(XEvent)]
display = x.XOpenDisplay(None)
if not display:
    sys.exit('cannot open display')
window = int(sys.argv[1])
event = XEvent()
event.xclient.type = 33  # ClientMessage
event.xclient.window = window
event.xclient.message_type = x.XInternAtom(display, b'WM_PROTOCOLS', 0)
event.xclient.format = 32
event.xclient.data[0] = x.XInternAtom(display, b'WM_DELETE_WINDOW', 0)
event.xclient.data[1] = 0  # CurrentTime
if not x.XSendEvent(display, window, 0, 0, ctypes.byref(event)):
    sys.exit('XSendEvent failed')
x.XFlush(display)
x.XCloseDisplay(display)
