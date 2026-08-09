using System.Text;

namespace AiWakeScheduler.Core;

public static class ArgumentTokenizer
{
    public static IReadOnlyList<string> Parse(string? commandLine)
    {
        if (string.IsNullOrWhiteSpace(commandLine))
        {
            return [];
        }

        var arguments = new List<string>();
        var current = new StringBuilder();
        var inQuotes = false;
        var backslashes = 0;

        foreach (var character in commandLine)
        {
            if (character == '\\')
            {
                backslashes++;
                continue;
            }

            if (character == '"')
            {
                current.Append('\\', backslashes / 2);
                if (backslashes % 2 == 1)
                {
                    current.Append('"');
                }
                else
                {
                    inQuotes = !inQuotes;
                }

                backslashes = 0;
                continue;
            }

            current.Append('\\', backslashes);
            backslashes = 0;

            if (char.IsWhiteSpace(character) && !inQuotes)
            {
                if (current.Length > 0)
                {
                    arguments.Add(current.ToString());
                    current.Clear();
                }
            }
            else
            {
                current.Append(character);
            }
        }

        current.Append('\\', backslashes);
        if (inQuotes)
        {
            throw new FormatException("額外參數含有未結束的雙引號。");
        }

        if (current.Length > 0)
        {
            arguments.Add(current.ToString());
        }

        return arguments;
    }
}

