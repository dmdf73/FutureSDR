from time import time
import numpy as np
import shutil
import matplotlib.pyplot as plt
from functions import get_coprimes, analyze_matches
import os
import csv
import subprocess
def compile_rust_projects():
    compile_commands = [
        "cargo build --release --bin samples",
        "cargo build --release --bin perf"
    ]
    for cmd in compile_commands:
        subprocess.run(cmd, shell=True, check=True)
def get_binary_path(bin_name):
    return os.path.join("..", "..", "target", "release", bin_name)

def run_command(command):
    try:
        result = subprocess.run(command, shell=True, check=True, capture_output=True, text=True)
    except subprocess.CalledProcessError as e:
        print(f"An error occurred: {e.stderr}")
        raise
    return
def run_single_analysis(sequence_length, snr_db, roots, threshold, use_fft):
    sync_root, wifi_root, lora_root, zigbee_root = roots
    if os.path.exists('matches.log'):
        os.remove('matches.log')
    if os.path.exists('output_after_noise.bin'):
        os.remove('output_after_noise.bin')
    samples_binary = get_binary_path("samples")
    samples_command = f"{samples_binary} --sequence-length {sequence_length} --sync-root {sync_root} --wifi-root {wifi_root} --lora-root {lora_root} --zigbee-root {zigbee_root} --snr-db={snr_db}"
    run_command(samples_command)
    perf_binary = get_binary_path("perf")
    start_time = time()
    fft_option = "--use-fft" if use_fft else ""
    perf_command = f"{perf_binary} --sequence-length {sequence_length} --sync-root {sync_root} --wifi-root {wifi_root} --lora-root {lora_root} --zigbee-root {zigbee_root} {fft_option} --threshold {threshold}"
    run_command(perf_command)
    end_time = time()
    run_time = (end_time - start_time)
    padding = 60
    # expected_sequence = [
    #     ("wifi", 20_636 + sequence_length * 2 + padding),
    #     ("lora", 23_866 + sequence_length * 2 + padding),
    #     ("zigbee", 82_541 + sequence_length * 2 + padding)   ]
    expected_sequence = [
        ("wifi", 30_000 + sequence_length * 2 + padding),
        ("lora", 30_000 + sequence_length * 2 + padding),
        ("zigbee", 30_000 + sequence_length * 2 + padding)   ]
    start_offset = 30
    max_offset = 1_000_000-sequence_length * 4 - padding
    false_positives, false_negatives, correct_matches = analyze_matches(
        expected_sequence,
        start_offset,
        max_offset,
        'matches.log'
    )
    true_positives = len(correct_matches)
    precision = true_positives / (true_positives + len(false_positives)) if (true_positives + len(false_positives)) > 0 else 1
    recall = true_positives / (true_positives + len(false_negatives)) if (true_positives + len(false_negatives)) > 0 else 1
    
    return {
        'SNR': snr_db,
        'Precision': precision * 100,
        'Recall': recall * 100,
        'True_Positives': true_positives,
        'False_Positives': len(false_positives),
        'False_Negatives': len(false_negatives),
        'Computing_Time': run_time,
        'FFT': use_fft
    }
def run_snr_analysis(sequence_length, snr_range, num_runs, threshold):
    roots = get_coprimes(sequence_length, 4)
    csv_filename = f'csv/snr_analysis_results_{sequence_length}.csv'
    if os.path.exists(csv_filename):
        os.remove(csv_filename)
    results = []
    fieldnames = ['SNR', 'Precision', 'Recall', 'True_Positives', 'False_Positives', 'False_Negatives', 'Computing_Time', 'FFT']
    
    with open(csv_filename, 'w', newline='') as csvfile:
        writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
        writer.writeheader()
    
    for snr_db in snr_range:
        print(f"Running analysis for SNR: {snr_db} dB")
        for run in range(num_runs):
            print(f"  Run {run + 1}/{num_runs}")
            
            for use_fft in [ True]:
                result = run_single_analysis(sequence_length, snr_db, roots, threshold, use_fft)
                
                with open(csv_filename, 'a', newline='') as csvfile:
                    writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
                    writer.writerow(result)
                
                fft_status = "with FFT" if use_fft else "without FFT"
                print(f"    {fft_status} - Precision: {result['Precision']:.2f}, Recall: {result['Recall']:.2f}")

            # if result['Precision'] < 60 or result['Recall'] < 60:
            #    exit()
    print(f"Results saved to {csv_filename}")
    return csv_filename
from scipy.interpolate import interp1d

def get_thresholds(needed_lengths, filename='Sequence_Threshold.csv'):
    lengths = []
    thresholds = []
    
    with open(filename, newline='') as csvfile:
        reader = csv.DictReader(csvfile)
        for row in reader:
            lengths.append(float(row['Sequence_Length']))
            thresholds.append(float(row['Threshold']) + 0.03)
            
    interp_func = interp1d(lengths, thresholds, kind='linear', fill_value='extrapolate')
    return [float(interp_func(length)) for length in needed_lengths]

sequence_lengths = np.linspace(50, 4000, 80, dtype=int)
snr_range = np.linspace(-20, 20, 20)
# threshold_range = np.linspace(0.07, 0.0, 100)
threshold_range = get_thresholds(sequence_lengths)
# threshold_range = np.linspace(0.5, 0.0, 100)
# sequence_lengths = np.linspace(76, 76, 1, dtype=int)
# sequence_lengths = np.linspace(5000, 5000, 1, dtype=int)
# snr_range = np.linspace(50, 100, 2)
num_runs = 5
# num_runs = 100
compile_rust_projects()
csv_dir = 'csv'
if os.path.exists(csv_dir):
    shutil.rmtree(csv_dir)
os.makedirs(csv_dir)
for sequence_length, threshold in zip(sequence_lengths, threshold_range):
    csv_filename = f'csv/snr_analysis_results_{sequence_length}.csv'
    print(f"Running analysis for Sequence Length: {sequence_length}, Threshold: {threshold:.2f}")
    run_snr_analysis(sequence_length, snr_range, num_runs, threshold)