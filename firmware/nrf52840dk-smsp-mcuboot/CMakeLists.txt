cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(smsp_mcuboot_slave)

target_sources(app PRIVATE
	src/main.c
	src/smsp_slave.c
	src/mcuboot_rpc.c
)
