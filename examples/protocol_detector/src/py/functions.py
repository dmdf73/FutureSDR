import subprocess
import time
from typing import List, Tuple, Dict
from dataclasses import dataclass

def run_command(command):
    start_time = time.time()
    subprocess.run(command, shell=True, check=True)
    end_time = time.time()
    return end_time - start_time

def gcd(a, b):
    while b:
        a, b = b, a % b
    return a

def get_coprimes(n, count):
    coprimes = []
    candidate = 2
    while len(coprimes) < count:
        if gcd(n, candidate) == 1:
            coprimes.append(candidate)
        candidate += 1
    return coprimes

@dataclass
class Sample:
    offset: int
    name: str

def read_samples(filename: str) -> List[Sample]:
    samples = []
    with open(filename, 'r') as file:
        for line in file:
            offset, name = line.strip().split(',')
            samples.append(Sample(int(offset), name))
    return samples

def generate_expected_samples(expected_sequence: List[Tuple[str, int]], 
                              start_offset: int, 
                              max_offset: int) -> List[Tuple[str, int]]:
    expected_samples = []
    current_offset = start_offset
    while current_offset < max_offset:
        for name, distance in expected_sequence:
            expected_samples.append((name, current_offset))
            current_offset += distance
            if current_offset >= max_offset:
                break
    return expected_samples

def analyze_matches(expected_sequence: List[Tuple[str, int]],
                    start_offset: int,
                    max_offset: int,
                    filename: str,
                    tolerance: int = 250) -> Tuple[List[Tuple[int, Sample]], List[Tuple[str, int]], Dict[int, Sample]]:
    expected_samples = generate_expected_samples(expected_sequence, start_offset, max_offset)
    actual_samples = read_samples(filename)
    
    false_positives = []
    false_negatives = expected_samples.copy()
    correct_matches = {}
    
    for sample in actual_samples:
        matched = False
        for expected in false_negatives:
            if sample.name == expected[0] and abs(sample.offset - expected[1]) <= tolerance:
                correct_matches[expected[1]] = sample
                false_negatives.remove(expected)
                matched = True
                break
        
        if not matched:
            false_positives.append((sample.offset, sample))
    
    return false_positives, false_negatives, correct_matches