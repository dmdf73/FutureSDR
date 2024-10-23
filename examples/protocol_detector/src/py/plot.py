import shutil
import pandas as pd
import os
import csv
import numpy as np
import matplotlib.pyplot as plt
import seaborn as sns
from collections import defaultdict
import scipy.stats as stats

def analyze_and_plot_results(csv_filename, sequence_length):
    with open(csv_filename, 'r') as csvfile:
        reader = csv.DictReader(csvfile)
        data = list(reader)
    
    snr_values = sorted(set(float(row['SNR']) for row in data))
    
    precision_data = {snr: [] for snr in snr_values}
    recall_data = {snr: [] for snr in snr_values}
    
    for row in data:
        snr = float(row['SNR'])
        precision_data[snr].append(float(row['Precision']))
        recall_data[snr].append(float(row['Recall']))
    
    precision_mean = [np.mean(precision_data[snr]) for snr in snr_values]
    recall_mean = [np.mean(recall_data[snr]) for snr in snr_values]
    
    precision_std = [np.std(precision_data[snr], ddof=1) for snr in snr_values]
    recall_std = [np.std(recall_data[snr], ddof=1) for snr in snr_values]
    
    plt.figure(figsize=(12, 8))
    plt.errorbar(snr_values, precision_mean, yerr=precision_std, fmt='bs-', capsize=5, label='Precision', linewidth=2, markersize=8)
    plt.errorbar(snr_values, recall_mean, yerr=recall_std, fmt='r^-', capsize=5, label='Recall', linewidth=2, markersize=8)
    plt.xlabel('SNR (dB)')
    plt.ylabel('Percentage (%)')
    plt.title(f'Precision and Recall vs SNR - Sequence Length: {sequence_length}')
    plt.legend()
    plt.grid(True)
    plt.ylim(0, 105)
    
    os.makedirs('plot', exist_ok=True)
    plt.savefig(f'plot/plot_sequence_length_{sequence_length}.png')
    plt.close()

def process_csv_folder(folder_path):
    for filename in os.listdir(folder_path):
        if filename.endswith('.csv'):
            sequence_length = filename.split('.')[0].split('_')[-1]
            csv_path = os.path.join(folder_path, filename)
            print(f"Processing file: {filename}")
            analyze_and_plot_results(csv_path, sequence_length)
            print(f"Plot saved for sequence length: {sequence_length}")

def load_csv_data(csv_folder):
    data = {}
    for filename in os.listdir(csv_folder):
        if filename.endswith('.csv'):
            sequence_length = int(filename.split('_')[-1].split('.')[0])
            with open(os.path.join(csv_folder, filename), 'r') as csvfile:
                reader = csv.DictReader(csvfile)
                for row in reader:
                    snr = float(row['SNR'])
                    precision = float(row['Precision'])
                    recall = float(row['Recall'])
                    f1_score = 2 * (precision * recall) / (precision + recall) if precision + recall > 0 else 0
                    if snr not in data:
                        data[snr] = {}
                    data[snr][sequence_length] = f1_score
    return data

def filter_outliers_zscore(data_map, threshold=3):
    filtered_data_map = {}
    for sequence_length, times in data_map.items():
        times_array = np.array(times)
        z_scores = np.abs((times_array - np.mean(times_array)) / np.std(times_array))
        filtered_times = times_array[z_scores < threshold]
        filtered_data_map[sequence_length] = filtered_times.tolist()
    return filtered_data_map

def plot_execution_time_vs_sequence_length(csv_folder, confidence_level=0.95):
    data_map = {'FFT': defaultdict(list), 'No FFT': defaultdict(list)}
    for filename in os.listdir(csv_folder):
        if filename.endswith('.csv'):
            sequence_length = int(filename.split('_')[-1].split('.')[0])
            csv_path = os.path.join(csv_folder, filename)
            
            with open(csv_path, 'r') as csvfile:
                reader = csv.DictReader(csvfile)
                for row in reader:
                    fft = 'FFT' if row['FFT'].lower() == 'true' else 'No FFT'
                    execution_time = float(row['Computing_Time'])
                    data_map[fft][sequence_length].append(execution_time)
    
    filtered_data_map = {fft: filter_outliers_zscore(data) for fft, data in data_map.items()}
    
    data = []
    for fft, lengths in filtered_data_map.items():
        for sequence_length, times in lengths.items():
            for time in times:
                data.append({'Sequence Length': sequence_length, 'Execution Time': time, 'FFT':fft})
    df = pd.DataFrame(data)
    
    plt.figure(figsize=(12, 8))
    sns.lineplot(x='Sequence Length', y='Execution Time', hue='FFT', data=df, 
                 err_style="band", linewidth=2.5,
                #  errorbar=('ci',confidence_level5) ,
                 err_kws={'alpha': 0.3, 'linewidth': 0})
    
    plt.xlabel('Sequence Length')
    plt.ylabel('Execution Time (s)')
    plt.title(f'Execution Time vs Sequence Length with {confidence_level*100}% Confidence Interval')
    plt.grid(True, which="both", ls="-", alpha=0.2)
    plt.legend(title='FFT Usage')
    
    plt.yscale('log')
    
    y_ticks = [10**i for i in range(-3, 4)]
    plt.yticks(y_ticks)
    
    plt.gca().yaxis.set_major_formatter(plt.FuncFormatter(lambda x, _: f'{x:.0e}' if x < 0.01 or x >= 100 else f'{x:.2f}'))
    
    plt.tight_layout()
    
    os.makedirs('plot', exist_ok=True)
    plt.savefig(f'plot/execution_time_vs_sequence_length_with_{int(confidence_level*100)}ci_fft_comparison_log_scale.png', dpi=1500)
    plt.close()
    
    print(f"Execution time plot with {confidence_level*100}% confidence interval and logarithmic scale has been generated.")

def find_required_sequence_length(csv_folder, threshold=99.0):
    data = defaultdict(lambda: defaultdict(list))
    
    for filename in os.listdir(csv_folder):
        if filename.endswith('.csv'):
            sequence_length = int(filename.split('_')[-1].split('.')[0])
            with open(os.path.join(csv_folder, filename), 'r') as csvfile:
                reader = csv.DictReader(csvfile)
                for row in reader:
                    snr = float(row['SNR'])
                    precision = float(row['Precision'])
                    recall = float(row['Recall'])
                    
                    data[snr][sequence_length].append((precision, recall))
    
    averaged_data = {}
    for snr, lengths in data.items():
        averaged_data[snr] = {}
        for length, values in lengths.items():
            avg_precision = sum(v[0] for v in values) / len(values)
            avg_recall = sum(v[1] for v in values) / len(values)
            averaged_data[snr][length] = (avg_precision, avg_recall)
    
    required_lengths = {}
    for snr, lengths in averaged_data.items():
        valid_lengths = [length for length, (precision, recall) in lengths.items() 
                         if precision >= threshold and recall >= threshold]
        if valid_lengths:
            required_lengths[snr] = min(valid_lengths)
        else:
            required_lengths[snr] = None
    
    return required_lengths

def plot_required_sequence_length(required_lengths):
    snr_values = sorted(required_lengths.keys())
    lengths = [required_lengths[snr] for snr in snr_values]

    plt.figure(figsize=(12, 8))
    
    valid_snr = [snr for snr, length in zip(snr_values, lengths) if length is not None]
    valid_lengths = [length for length in lengths if length is not None]
    plt.plot(valid_snr, valid_lengths, 'bo-', label='Required Sequence Length')
    
    invalid_snr = [snr for snr, length in zip(snr_values, lengths) if length is None]
    plt.plot(invalid_snr, [plt.ylim()[1]] * len(invalid_snr), 'rx', markersize=10, 
             label='No Valid Sequence Length')

    plt.xlabel('SNR (dB)')
    plt.ylabel('Required Sequence Length')
    plt.title('Required Sequence Length for Precision and Recall > 99% vs. SNR')
    plt.legend()
    plt.grid(True)
    
    # plt.yscale('log')
    
    plt.tight_layout()
    plt.savefig('plot/required_sequence_length_vs_snr.png')
    plt.close()

# Main execution
folder_name = "plot"
if os.path.exists(folder_name):
    shutil.rmtree(folder_name)
os.makedirs(folder_name)

csv_folder = 'csv'
process_csv_folder(csv_folder)
# plot_f1_score_heatmap(csv_folder)
plot_execution_time_vs_sequence_length(csv_folder)

required_lengths = find_required_sequence_length(csv_folder)
plot_required_sequence_length(required_lengths)

print("All plots have been generated.")