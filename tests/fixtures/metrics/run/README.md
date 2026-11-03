A cut-down release run for checks/tests/metrics.rs (tests/metrics.sh). Lines are trimmed copies of
the v0.12 run on the forge (/scratch/rel012, 2026-11-03) plus the files that run did not have
(install-gui pages, idle.txt, the WOL_SHELL_TIMING line), so every metric has a known value here:
boot_s 37.0, boot_systemd_s 39.2, install_s 151.4, pages 3, idle_cpu_ms_s 0.34, shell_map_ms 237.4,
iso_bytes 2128447488, journeys 2 of 3 passed.
