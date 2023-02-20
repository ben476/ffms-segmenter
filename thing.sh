for file in /out/*
do
  ffmpeg -i "$file" "${file%.*}.png"
done