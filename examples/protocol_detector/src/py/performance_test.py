import statistics
import matplotlib.pyplot as plt
import csv
import numpy as np
from functions import run_command, get_coprimes

sequence_lengths = range(30, 1000, 5)
results = {'fft': [], 'no_fft': []}

for length in sequence_lengths:
    print(f"Testing sequence length: {length}")
    
    roots = get_coprimes(length, 4)
    sync_root, wifi_root, lora_root, zigbee_root = roots
    
    samples_command = f"cargo run --release --bin samples -- --sequence-length {length} --sync-root {sync_root} --wifi-root {wifi_root} --lora-root {lora_root} --zigbee-root {zigbee_root}"
    run_command(samples_command)
    
    for use_fft in [True, False]:
        fft_key = 'fft' if use_fft else 'no_fft'
        perf_times = []
        
        for _ in range(10):
            perf_command = f"cargo run --release --bin perf -- --sequence-length {length} --sync-root {sync_root} --wifi-root {wifi_root} --lora-root {lora_root} --zigbee-root {zigbee_root} {'--use-fft' if use_fft else ''}"
            perf_time = run_command(perf_command)
            perf_times.append(perf_time)
        
        avg_perf_time = statistics.mean(perf_times)
        std_dev = statistics.stdev(perf_times)
        
        # Berechnung des 95% Konfidenzintervalls
        confidence_interval = 1.96 * (std_dev / np.sqrt(len(perf_times)))
        
        results[fft_key].append({
            "Sequence Length": length,
            "Average Time": avg_perf_time,
            "Std Dev": std_dev,
            "Confidence Interval": confidence_interval
        })
        print(f"FFT: {use_fft}, Average performance time: {avg_perf_time:.4f} seconds (Std Dev: {std_dev:.4f}, CI: ±{confidence_interval:.4f})")
    
    print()  # Leerzeile für bessere Lesbarkeit

# Daten für das Plotten vorbereiten
fft_times = [result["Average Time"] for result in results['fft']]
no_fft_times = [result["Average Time"] for result in results['no_fft']]
fft_ci = [result["Confidence Interval"] for result in results['fft']]
no_fft_ci = [result["Confidence Interval"] for result in results['no_fft']]

plt.figure(figsize=(12, 6))
plt.errorbar(sequence_lengths, fft_times, yerr=fft_ci, fmt='o-', capsize=5, capthick=1, color='blue', ecolor='lightblue', label='With FFT')
plt.errorbar(sequence_lengths, no_fft_times, yerr=no_fft_ci, fmt='o-', capsize=5, capthick=1, color='red', ecolor='lightcoral', label='Without FFT')
plt.title("Average Performance Time vs Sequence Length with 95% Confidence Intervals")
plt.xlabel("Sequence Length")
plt.ylabel("Average Performance Time (seconds)")
plt.grid(True)
plt.legend()
plt.savefig("performance_results_fft_comparison.png")
plt.close()

with open("performance_results_fft_comparison.csv", "w", newline='') as csvfile:
    fieldnames = ["Sequence Length", "Average Time (FFT)", "Std Dev (FFT)", "CI (FFT)", "Average Time (No FFT)", "Std Dev (No FFT)", "CI (No FFT)"]
    writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
    
    writer.writeheader()
    for fft_result, no_fft_result in zip(results['fft'], results['no_fft']):
        writer.writerow({
            "Sequence Length": fft_result["Sequence Length"],
            "Average Time (FFT)": fft_result["Average Time"],
            "Std Dev (FFT)": fft_result["Std Dev"],
            "CI (FFT)": fft_result["Confidence Interval"],
            "Average Time (No FFT)": no_fft_result["Average Time"],
            "Std Dev (No FFT)": no_fft_result["Std Dev"],
            "CI (No FFT)": no_fft_result["Confidence Interval"]
        })

print("Performance test completed. Results saved in 'performance_results_fft_comparison.png' and 'performance_results_fft_comparison.csv'")