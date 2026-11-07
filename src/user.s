// Programa de EL0 e a ida e volta com o kernel.
//
// user_blob recebe em x0 o VA dos dados e em x1 o VA do kernel a sondar.
// Grava 0x55, lê de volta e deixa 1 no deslocamento 8 se os dois bateram.
// Zera NZCV para o SPSR do abort não depender das flags, e então lê o kernel.
//
// user_enter grava os callee-saved, arma ELR_EL1, SPSR_EL1 e SP_EL0 e faz
// ERET. A falta volta por user_kernel_return, que restaura o quadro e
// devolve o controle a quem chamou user_enter.

.section .text
.global user_enter
.type user_enter, %function
// x0 = VA de entrada, x1 = topo da pilha EL0, x2 = VA dos dados, x3 = sonda
user_enter:
    msr     daifset, #2
    sub     sp, sp, #96
    stp     x19, x20, [sp, #0]
    stp     x21, x22, [sp, #16]
    stp     x23, x24, [sp, #32]
    stp     x25, x26, [sp, #48]
    stp     x27, x28, [sp, #64]
    stp     x29, x30, [sp, #80]
    adr     x9, user_saved_sp
    mov     x10, sp
    str     x10, [x9]

    msr     elr_el1, x0
    msr     sp_el0, x1
    mov     x9, #0x3c0
    msr     spsr_el1, x9
    mov     x0, x2
    mov     x1, x3
    isb
    eret

.global user_kernel_return
.type user_kernel_return, %function
user_kernel_return:
    adr     x9, user_saved_sp
    ldr     x10, [x9]
    mov     sp, x10
    ldp     x19, x20, [sp, #0]
    ldp     x21, x22, [sp, #16]
    ldp     x23, x24, [sp, #32]
    ldp     x25, x26, [sp, #48]
    ldp     x27, x28, [sp, #64]
    ldp     x29, x30, [sp, #80]
    add     sp, sp, #96
    ret

.section .rodata
.global user_blob
.global user_blob_end
.balign 4
user_blob:
    mov     x4, #0x55
    str     x4, [x0]
    ldr     x5, [x0]
    cmp     x5, x4
    cset    w5, eq
    str     w5, [x0, #8]
    msr     nzcv, xzr
    ldr     x6, [x1]
    svc     #0
user_blob_end:

.global syscall_blob
.global syscall_blob_end
.balign 4
// x0 = VA dos dados. x8 escolhe a chamada: 0 escreve, 1 lê, 2 tempo, 3 encerra.
// O deslocamento 0 guarda a escrita recusada, o 16 o CLO e o 24 o byte lido.
syscall_blob:
    mov     x19, x0
    movz    x0, #8, lsl #16
    mov     x1, #1
    mov     x8, #0
    svc     #0
    str     x0, [x19]
    adr     x0, syscall_msg
    mov     x1, #11
    mov     x8, #0
    svc     #0
    mov     x8, #2
    svc     #0
    str     x0, [x19, #16]
    add     x0, x19, #24
    mov     x1, #1
    mov     x8, #1
    svc     #0
    mov     x0, #0
    mov     x8, #3
    svc     #0
syscall_msg:
    .ascii  "saida: ok\r\n"
syscall_blob_end:

.section .data
.balign 8
user_saved_sp:
    .quad 0
