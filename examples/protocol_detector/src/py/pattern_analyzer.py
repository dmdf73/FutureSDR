def analyze_pattern(filename):
    expected_sequence = ['wifi', 'lora', 'zigbee']
    false_positives = []
    false_negatives = []
    expected_index = 0

    try:
        with open(filename, 'r') as file:
            data = file.read().strip().split(',')

        for i in range(0, len(data), 2):
            current_tech = data[i+1].lower() if i+1 < len(data) else None
            
            if current_tech != expected_sequence[expected_index]:
                false_positives.append(f"{data[i]},{current_tech}")
            else:
                expected_index = (expected_index + 1) % 3

        if expected_index != 0:
            false_negatives = expected_sequence[expected_index:]

        return false_positives, false_negatives

    except FileNotFoundError:
        return [], [], f"Error: File '{filename}' not found."
    except Exception as e:
        return [], [], f"Error: An unexpected error occurred: {str(e)}"