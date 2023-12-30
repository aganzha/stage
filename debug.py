import gdb

def set_breakpoints():
    p1 = gdb.Breakpoint('src/text_view.rs:508')
    p2 = gdb.Breakpoint('src/text_view.rs:542')
    print('breakpoints', p1, p2)

set_breakpoints()

