// Tabela de vetores AArch64. VBAR_ELx exige alinhamento de 2 KiB.
// Cada entrada tem 128 bytes. Os quatro grupos são SP0, SPx, EL inferior
// AArch64 e EL inferior AArch32. O núcleo usa SP_ELx, então a síncrona
// cai em 0x200, mas os outros slots apontam para o mesmo relato.

.section .text.vectors, "ax"
.balign 2048
.global exception_vectors
.type exception_vectors, %function

.macro vector_entry kind
    sub     sp, sp, #288
    stp     x0, x1, [sp, #0]
    add     x0, sp, #288
    str     x0, [sp, #280]
    mov     x0, #\kind
    b       exception_common
.endm

exception_vectors:
    vector_entry 0
    .balign 128
    vector_entry 1
    .balign 128
    vector_entry 2
    .balign 128
    vector_entry 3
    .balign 128
    vector_entry 0
    .balign 128
    vector_entry 1
    .balign 128
    vector_entry 2
    .balign 128
    vector_entry 3
    .balign 128
    vector_entry 0
    .balign 128
    vector_entry 1
    .balign 128
    vector_entry 2
    .balign 128
    vector_entry 3
    .balign 128
    vector_entry 0
    .balign 128
    vector_entry 1
    .balign 128
    vector_entry 2
    .balign 128
    vector_entry 3

// x0 = tipo (0 síncrona, 1 irq, 2 fiq, 3 serror).
// [sp, #280] = SP de antes do quadro. x0–x1 já estão salvos.
exception_common:
    stp     x2, x3, [sp, #16]
    stp     x4, x5, [sp, #32]
    stp     x6, x7, [sp, #48]
    stp     x8, x9, [sp, #64]
    stp     x10, x11, [sp, #80]
    stp     x12, x13, [sp, #96]
    stp     x14, x15, [sp, #112]
    stp     x16, x17, [sp, #128]
    stp     x18, x19, [sp, #144]
    stp     x20, x21, [sp, #160]
    stp     x22, x23, [sp, #176]
    stp     x24, x25, [sp, #192]
    stp     x26, x27, [sp, #208]
    stp     x28, x29, [sp, #224]
    str     x30, [sp, #240]
    mov     x21, x0

    mrs     x1, currentel
    cmp     x1, #0x8
    b.eq    read_el2
    cmp     x1, #0xc
    b.eq    read_el3
    mrs     x2, esr_el1
    mrs     x3, elr_el1
    mrs     x4, spsr_el1
    mrs     x5, far_el1
    b       store_sys
read_el2:
    mrs     x2, esr_el2
    mrs     x3, elr_el2
    mrs     x4, spsr_el2
    mrs     x5, far_el2
    b       store_sys
read_el3:
    mrs     x2, esr_el3
    mrs     x3, elr_el3
    mrs     x4, spsr_el3
    mrs     x5, far_el3
store_sys:
    str     x3, [sp, #248]
    str     x4, [sp, #256]
    str     x2, [sp, #264]
    str     x5, [sp, #272]

    mov     x0, sp
    mov     x1, x21
    ldr     x2, [sp, #280]
    bl      exception_entry

    mrs     x1, currentel
    ldr     x2, [sp, #248]
    ldr     x3, [sp, #256]
    cmp     x1, #0x8
    b.eq    restore_el2
    cmp     x1, #0xc
    b.eq    restore_el3
    msr     elr_el1, x2
    msr     spsr_el1, x3
    b       restore_gpr
restore_el2:
    msr     elr_el2, x2
    msr     spsr_el2, x3
    b       restore_gpr
restore_el3:
    msr     elr_el3, x2
    msr     spsr_el3, x3
restore_gpr:
    ldp     x0, x1, [sp, #0]
    ldp     x2, x3, [sp, #16]
    ldp     x4, x5, [sp, #32]
    ldp     x6, x7, [sp, #48]
    ldp     x8, x9, [sp, #64]
    ldp     x10, x11, [sp, #80]
    ldp     x12, x13, [sp, #96]
    ldp     x14, x15, [sp, #112]
    ldp     x16, x17, [sp, #128]
    ldp     x18, x19, [sp, #144]
    ldp     x20, x21, [sp, #160]
    ldp     x22, x23, [sp, #176]
    ldp     x24, x25, [sp, #192]
    ldp     x26, x27, [sp, #208]
    ldp     x28, x29, [sp, #224]
    ldr     x30, [sp, #240]
    add     sp, sp, #288
    eret
