
#include <string.h>
#include <stdio.h>

void _print(const char *m, int len)
{
    __asm__("mr 5, 4\n"
            "mr 4, 3\n"
            "li 3, 1\n"
            "li 0, 4\n"
            "sc");
}

void print(const char *m)
{
    _print(m, strlen(m));
}

__attribute__((noinline)) int a()
{
    __asm__("nop");
    return 1;
}
__attribute__((noinline)) int b()
{
    __asm__("nop");
    return 1;
}
__attribute__((noinline)) int c()
{
    __asm__("nop");
    return 1;
}
__attribute__((noinline)) int d(int v)
{
    __asm__("nop");
    return 1;
}
__attribute__((noinline)) void e(int v)
{
    __asm__("nop");
}


int test2(int x) {
    if (x == 3) {
        a();
    } else {
        if (x == 4) {
            b();
        }
    }
    return d(1);
}

// TODO: some bugs here, investigate::
// int switch_p(int x) {
//     switch(x) {
//         case 1: return 15;
//         case 2: return 35;
//         case 3: return 60;
//         case 4: return 80;
//     }
// }
//

// TODO: some bugs here (crashes), investigate::
//
// int test(int x, int y) {
//     for (int i = x; i < y; i++) {
//         if (a()) {
//              d(i);
//         } else {
//              e(i);
//         }
//     }
//     d(x);
//     d(y);
// }

__attribute__((section(".test")))
int test(int x, int y) {
    for (int i = 0; i < 10; i++) {
        d(i);
    }
    // for (int i = x; i < y; i++) {
    //     d(i);
    // }

    // d(x);
    // d(y);
}

void _start()
{
    test(0, 5);
    // __asm__("mov $60, %eax\n"
    //         "mov $42, %edi\n"
    //         "syscall");

    __asm__("li 0, 1\n"
            "li 3, 42\n"
            "sc");
}
