demo.elf: demo.o
	arm-none-eabi-ld $< -o $@

demo.o: demo.s
	arm-none-eabi-as -mcpu=cortex-m33 -mfpu=fpv5-sp-d16 -mthumb $< -o $@

